//! WAL文件持久化和启动恢复。

use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{AppError, Result},
    protocol::MAX_FRAME_BYTES,
    storage::{SetOutcome, Store, StoreStats, validate_key, validate_value},
};

/// WAL中允许出现的修改操作。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
enum WalRecord {
    Set { key: String, value: String },
    Delete { key: String },
}

/// WAL中的完整记录，CRC32用于检查操作内容是否被修改。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WalEntry {
    record: WalRecord,
    crc32: String,
}

/// 持久化存储状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersistentStats {
    /// 当前内存数据的统计信息。
    pub store: StoreStats,
    /// WAL中的修改记录数。
    pub wal_records: u64,
    /// WAL文件大小，单位为字节。
    pub wal_bytes: u64,
    /// 是否允许继续修改数据。
    pub writable: bool,
}

/// 内存数据和WAL文件的统一入口。
#[derive(Debug)]
pub struct PersistentStore {
    store: Store,         // 内存中的键值数据
    wal: BufWriter<File>, // WAL文件写入器
    wal_path: PathBuf,    // WAL文件路径
    writable: bool,       // 是否允许写入
    wal_records: u64,     // WAL记录数
    wal_bytes: u64,       // WAL文件大小
}

impl PersistentStore {
    /// 打开WAL并恢复全部数据。
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let wal_path = path.as_ref().to_path_buf();
        if let Some(parent) = wal_path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }

        // append模式只负责创建空文件，不会截断已有内容。
        drop(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&wal_path)?,
        );

        let (store, wal_records) = recover(&wal_path)?;
        let wal_bytes = fs::metadata(&wal_path)?.len();
        let wal = BufWriter::new(OpenOptions::new().append(true).open(&wal_path)?);

        Ok(Self {
            store,
            wal,
            wal_path,
            writable: true,
            wal_records,
            wal_bytes,
        })
    }

    /// 先写WAL，再更新内存。
    pub fn set(&mut self, key: String, value: String) -> Result<SetOutcome> {
        self.ensure_writable()?;
        validate_key(&key)?;
        validate_value(&value)?;

        let record = WalRecord::Set {
            key: key.clone(),
            value: value.clone(),
        };
        self.append_record(&record)?;

        Ok(self.store.set_validated(key, value))
    }

    pub fn get(&self, key: &str) -> Result<&str> {
        self.store.get(key)
    }

    /// 先写WAL，再删除内存数据。
    pub fn delete(&mut self, key: &str) -> Result<String> {
        self.ensure_writable()?;
        validate_key(key)?;
        let old_value = self.store.get(key)?.to_owned();

        let record = WalRecord::Delete {
            key: key.to_owned(),
        };
        self.append_record(&record)?;

        let removed = self.store.delete_validated(key);
        debug_assert_eq!(removed.as_deref(), Some(old_value.as_str()));
        Ok(old_value)
    }

    pub fn keys(&self) -> Vec<String> {
        self.store.keys()
    }

    pub fn len(&self) -> usize {
        self.store.len()
    }

    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }

    pub fn stats(&self) -> PersistentStats {
        PersistentStats {
            store: self.store.stats(),
            wal_records: self.wal_records,
            wal_bytes: self.wal_bytes,
            writable: self.writable,
        }
    }

    pub fn wal_path(&self) -> &Path {
        &self.wal_path
    }

    fn ensure_writable(&self) -> Result<()> {
        if self.writable {
            Ok(())
        } else {
            Err(AppError::storage("WAL先前写入失败，存储已进入只读状态"))
        }
    }

    fn append_record(&mut self, record: &WalRecord) -> Result<()> {
        let encoded = encode_entry(record)?;
        if encoded.len() > MAX_FRAME_BYTES {
            return Err(AppError::storage(format!(
                "WAL记录为 {} 字节，最大允许 {MAX_FRAME_BYTES} 字节",
                encoded.len()
            )));
        }

        let next_record_count = self
            .wal_records
            .checked_add(1)
            .ok_or_else(|| AppError::storage("WAL记录数量溢出"))?;
        let written_bytes = encoded
            .len()
            .checked_add(1)
            .and_then(|length| u64::try_from(length).ok())
            .ok_or_else(|| AppError::storage("WAL记录长度溢出"))?;
        let next_wal_bytes = self
            .wal_bytes
            .checked_add(written_bytes)
            .ok_or_else(|| AppError::storage("WAL文件大小计数溢出"))?;

        let write_result: io::Result<()> = (|| {
            self.wal.write_all(&encoded)?;
            self.wal.write_all(b"\n")?;
            self.wal.flush()?;
            self.wal.get_ref().sync_data()?;
            Ok(())
        })();

        if let Err(error) = write_result {
            self.writable = false;
            return Err(AppError::Io(error));
        }

        self.wal_records = next_record_count;
        self.wal_bytes = next_wal_bytes;
        Ok(())
    }
}

fn recover(path: &Path) -> Result<(Store, u64)> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut store = Store::new();
    let mut line_bytes = Vec::new();
    let mut line_number = 0_usize;
    let mut record_count = 0_u64;

    loop {
        line_bytes.clear();
        let bytes_read = reader.read_until(b'\n', &mut line_bytes)?;
        if bytes_read == 0 {
            break;
        }
        line_number += 1;

        let has_lf = line_bytes.last() == Some(&b'\n');
        let payload_length = line_bytes.len() - usize::from(has_lf);
        if payload_length > MAX_FRAME_BYTES {
            return Err(AppError::corrupt_wal(
                line_number,
                format!("记录为 {payload_length} 字节，最大允许 {MAX_FRAME_BYTES} 字节"),
            ));
        }
        if !has_lf {
            return Err(AppError::corrupt_wal(
                line_number,
                "记录缺少结尾LF，文件可能被截断",
            ));
        }

        let payload = &line_bytes[..payload_length];
        if payload.iter().all(u8::is_ascii_whitespace) {
            return Err(AppError::corrupt_wal(line_number, "记录不能为空"));
        }

        let entry: WalEntry = serde_json::from_slice(payload).map_err(|error| {
            AppError::corrupt_wal(line_number, format!("JSON格式错误：{error}"))
        })?;
        verify_checksum(&entry, line_number)?;
        replay_record(&mut store, entry.record, line_number)?;
        record_count = record_count
            .checked_add(1)
            .ok_or_else(|| AppError::storage("WAL记录数量溢出"))?;
    }

    Ok((store, record_count))
}

fn encode_entry(record: &WalRecord) -> Result<Vec<u8>> {
    let record_bytes = serde_json::to_vec(record)
        .map_err(|error| AppError::storage(format!("WAL操作序列化失败：{error}")))?;
    let entry = WalEntry {
        record: record.clone(),
        crc32: format!("{:08X}", crc32fast::hash(&record_bytes)),
    };

    serde_json::to_vec(&entry)
        .map_err(|error| AppError::storage(format!("WAL记录序列化失败：{error}")))
}

fn verify_checksum(entry: &WalEntry, line: usize) -> Result<()> {
    if entry.crc32.len() != 8 || !entry.crc32.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::corrupt_wal(
            line,
            format!("CRC32格式错误，应为8位十六进制数：{}", entry.crc32),
        ));
    }

    let stored = u32::from_str_radix(&entry.crc32, 16).map_err(|_| {
        AppError::corrupt_wal(
            line,
            format!("CRC32格式错误，应为8位十六进制数：{}", entry.crc32),
        )
    })?;
    let record_bytes = serde_json::to_vec(&entry.record)
        .map_err(|error| AppError::corrupt_wal(line, format!("WAL操作序列化失败：{error}")))?;
    let actual = crc32fast::hash(&record_bytes);

    if stored != actual {
        return Err(AppError::corrupt_wal(
            line,
            format!(
                "CRC32校验失败：文件记录为 {:08X}，重新计算为 {actual:08X}",
                stored
            ),
        ));
    }

    Ok(())
}

fn replay_record(store: &mut Store, record: WalRecord, line: usize) -> Result<()> {
    match record {
        WalRecord::Set { key, value } => {
            validate_key(&key)
                .map_err(|error| AppError::corrupt_wal(line, error.client_message()))?;
            validate_value(&value)
                .map_err(|error| AppError::corrupt_wal(line, error.client_message()))?;
            store.set_validated(key, value);
        }
        WalRecord::Delete { key } => {
            validate_key(&key)
                .map_err(|error| AppError::corrupt_wal(line, error.client_message()))?;
            if store.delete_validated(&key).is_none() {
                return Err(AppError::corrupt_wal(
                    line,
                    format!("删除的键不存在：{key}"),
                ));
            }
        }
    }
    Ok(())
}
