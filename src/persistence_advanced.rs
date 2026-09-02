//! 带版本、序号和快照压缩的持久化实现。

use std::{
    ffi::OsString,
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

const WAL_VERSION: u8 = 1;
const SNAPSHOT_VERSION: u8 = 1;
const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;

/// WAL中允许出现的修改操作。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
enum WalRecord {
    Set { key: String, value: String },
    Delete { key: String },
}

/// 新版WAL校验的实际内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WalPayload {
    version: u8,
    seq: u64,
    record: WalRecord,
}

/// 新版WAL记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WalEntry {
    payload: WalPayload,
    crc32: String,
}

/// 基础版WAL格式，用于平滑升级已有数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyWalEntry {
    record: WalRecord,
    crc32: String,
}

#[derive(Debug)]
enum DiskWalEntry {
    Current(WalEntry),
    Legacy(LegacyWalEntry),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotEntry {
    key: String,
    value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotPayload {
    version: u8,
    last_seq: u64,
    entries: Vec<SnapshotEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotFile {
    payload: SnapshotPayload,
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

/// 一次压缩前后的文件统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionStats {
    pub records_before: u64,
    pub wal_bytes_before: u64,
    pub records_after: u64,
    pub wal_bytes_after: u64,
    pub snapshot_entries: usize,
    pub snapshot_bytes: u64,
    pub last_seq: u64,
}

/// 内存数据和持久化文件的统一入口。
#[derive(Debug)]
pub struct PersistentStore {
    store: Store,
    wal: Option<BufWriter<File>>,
    wal_path: PathBuf,
    snapshot_path: PathBuf,
    writable: bool,
    wal_records: u64,
    wal_bytes: u64,
    last_seq: u64,
}

impl PersistentStore {
    /// 先加载快照，再按顺序恢复WAL。
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let wal_path = path.as_ref().to_path_buf();
        if let Some(parent) = wal_path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }

        let snapshot_path = wal_path.with_extension("snapshot");
        restore_snapshot_backup(&snapshot_path)?;

        // append模式只负责创建空文件，不会截断已有内容。
        drop(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&wal_path)?,
        );

        let (store, snapshot_seq) = recover_snapshot(&snapshot_path)?;
        let recovered = recover_wal(&wal_path, store, snapshot_seq)?;
        let wal_bytes = fs::metadata(&wal_path)?.len();
        let wal = BufWriter::new(OpenOptions::new().append(true).open(&wal_path)?);

        Ok(Self {
            store: recovered.store,
            wal: Some(wal),
            wal_path,
            snapshot_path,
            writable: true,
            wal_records: recovered.record_count,
            wal_bytes,
            last_seq: recovered.last_seq,
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

    pub fn snapshot_path(&self) -> &Path {
        &self.snapshot_path
    }

    pub fn last_sequence(&self) -> u64 {
        self.last_seq
    }

    /// 把当前状态写成快照，成功后清空旧WAL。
    pub fn compact(&mut self) -> Result<CompactionStats> {
        self.ensure_writable()?;

        let records_before = self.wal_records;
        let wal_bytes_before = self.wal_bytes;
        self.close_wal()?;

        let compact_result = self.compact_closed(records_before, wal_bytes_before);
        let reopen_result = OpenOptions::new().append(true).open(&self.wal_path);

        match reopen_result {
            Ok(file) => {
                self.wal = Some(BufWriter::new(file));
                self.writable = true;
            }
            Err(error) => {
                self.writable = false;
                return match compact_result {
                    Ok(_) => Err(AppError::Io(error)),
                    Err(compact_error) => Err(AppError::storage(format!(
                        "压缩失败：{compact_error}；重新打开WAL也失败：{error}"
                    ))),
                };
            }
        }

        match compact_result {
            Ok(stats) => {
                self.wal_records = 0;
                self.wal_bytes = 0;
                Ok(stats)
            }
            Err(error) => Err(error),
        }
    }

    fn ensure_writable(&self) -> Result<()> {
        if self.writable && self.wal.is_some() {
            Ok(())
        } else {
            Err(AppError::storage("WAL先前写入失败，存储已进入只读状态"))
        }
    }

    fn append_record(&mut self, record: &WalRecord) -> Result<()> {
        let next_seq = self
            .last_seq
            .checked_add(1)
            .ok_or_else(|| AppError::storage("WAL序号溢出"))?;
        let encoded = encode_entry(next_seq, record)?;
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
            let wal = self
                .wal
                .as_mut()
                .ok_or_else(|| io::Error::other("WAL写入器未打开"))?;
            wal.write_all(&encoded)?;
            wal.write_all(b"\n")?;
            wal.flush()?;
            wal.get_ref().sync_data()?;
            Ok(())
        })();

        if let Err(error) = write_result {
            self.writable = false;
            return Err(AppError::Io(error));
        }

        self.wal_records = next_record_count;
        self.wal_bytes = next_wal_bytes;
        self.last_seq = next_seq;
        Ok(())
    }

    fn close_wal(&mut self) -> Result<()> {
        let mut wal = self
            .wal
            .take()
            .ok_or_else(|| AppError::storage("WAL写入器未打开"))?;
        if let Err(error) = wal.flush().and_then(|_| wal.get_ref().sync_data()) {
            self.writable = false;
            return Err(AppError::Io(error));
        }
        Ok(())
    }

    fn compact_closed(
        &self,
        records_before: u64,
        wal_bytes_before: u64,
    ) -> Result<CompactionStats> {
        let entries = self
            .store
            .snapshot_entries()
            .into_iter()
            .map(|(key, value)| SnapshotEntry { key, value })
            .collect::<Vec<_>>();
        let payload = SnapshotPayload {
            version: SNAPSHOT_VERSION,
            last_seq: self.last_seq,
            entries,
        };
        let snapshot_bytes = encode_snapshot(&payload)?;
        let temp_path = path_with_suffix(&self.snapshot_path, ".tmp");
        let backup_path = path_with_suffix(&self.snapshot_path, ".bak");

        if temp_path.exists() {
            fs::remove_file(&temp_path)?;
        }

        {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)?;
            let mut writer = BufWriter::new(file);
            writer.write_all(&snapshot_bytes)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
            writer.get_ref().sync_data()?;
        }

        // 发布前重新读取一次，避免把不完整快照替换成正式文件。
        let (verified_store, verified_seq) = load_snapshot_file(&temp_path)?;
        if verified_seq != self.last_seq
            || verified_store.snapshot_entries() != self.store.snapshot_entries()
        {
            return Err(AppError::storage("快照写入后的校验结果与内存不一致"));
        }

        publish_snapshot(&temp_path, &self.snapshot_path, &backup_path)?;

        let wal_file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.wal_path)?;
        wal_file.sync_data()?;

        if backup_path.exists() {
            let _ = fs::remove_file(&backup_path);
        }

        let snapshot_bytes = fs::metadata(&self.snapshot_path)?.len();
        Ok(CompactionStats {
            records_before,
            wal_bytes_before,
            records_after: 0,
            wal_bytes_after: 0,
            snapshot_entries: self.store.len(),
            snapshot_bytes,
            last_seq: self.last_seq,
        })
    }
}

struct WalRecovery {
    store: Store,
    record_count: u64,
    last_seq: u64,
}

fn recover_wal(path: &Path, mut store: Store, snapshot_seq: u64) -> Result<WalRecovery> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut line_bytes = Vec::new();
    let mut line_number = 0_usize;
    let mut record_count = 0_u64;
    let mut sequence_cursor = None;
    let mut current_format_seen = false;

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

        let line = &line_bytes[..payload_length];
        if line.iter().all(u8::is_ascii_whitespace) {
            return Err(AppError::corrupt_wal(line_number, "记录不能为空"));
        }

        let entry = parse_wal_entry(line, line_number)?;
        let (seq, record) = match entry {
            DiskWalEntry::Legacy(entry) => {
                if current_format_seen {
                    return Err(AppError::corrupt_wal(
                        line_number,
                        "新版WAL后不能再出现旧版记录",
                    ));
                }
                verify_wal_checksum(&entry.crc32, &entry.record, line_number)?;
                let seq = record_count
                    .checked_add(1)
                    .ok_or_else(|| AppError::storage("WAL序号溢出"))?;
                sequence_cursor = Some(seq);
                (seq, entry.record)
            }
            DiskWalEntry::Current(entry) => {
                current_format_seen = true;
                if entry.payload.version != WAL_VERSION {
                    return Err(AppError::corrupt_wal(
                        line_number,
                        format!("不支持的WAL版本：{}", entry.payload.version),
                    ));
                }
                verify_payload_checksum(&entry, line_number)?;
                validate_sequence(
                    entry.payload.seq,
                    sequence_cursor,
                    snapshot_seq,
                    line_number,
                )?;
                sequence_cursor = Some(entry.payload.seq);
                (entry.payload.seq, entry.payload.record)
            }
        };

        if seq > snapshot_seq {
            replay_record(&mut store, record, line_number)?;
        }
        record_count = record_count
            .checked_add(1)
            .ok_or_else(|| AppError::storage("WAL记录数量溢出"))?;
    }

    Ok(WalRecovery {
        store,
        record_count,
        last_seq: sequence_cursor.unwrap_or(snapshot_seq).max(snapshot_seq),
    })
}

fn parse_wal_entry(line: &[u8], line_number: usize) -> Result<DiskWalEntry> {
    let value: serde_json::Value = serde_json::from_slice(line)
        .map_err(|error| AppError::corrupt_wal(line_number, format!("JSON格式错误：{error}")))?;

    if value.get("payload").is_some() {
        serde_json::from_value(value)
            .map(DiskWalEntry::Current)
            .map_err(|error| AppError::corrupt_wal(line_number, format!("JSON格式错误：{error}")))
    } else {
        serde_json::from_value(value)
            .map(DiskWalEntry::Legacy)
            .map_err(|error| AppError::corrupt_wal(line_number, format!("JSON格式错误：{error}")))
    }
}

fn validate_sequence(
    seq: u64,
    previous: Option<u64>,
    snapshot_seq: u64,
    line: usize,
) -> Result<()> {
    let valid = match previous {
        Some(previous) => previous.checked_add(1) == Some(seq),
        None => seq == 1 || snapshot_seq.checked_add(1) == Some(seq),
    };
    if valid {
        Ok(())
    } else {
        let expected = previous.and_then(|value| value.checked_add(1)).map_or_else(
            || format!("1或{}", snapshot_seq.saturating_add(1)),
            |value| value.to_string(),
        );
        Err(AppError::corrupt_wal(
            line,
            format!("WAL序号不连续：期望{expected}，实际{seq}"),
        ))
    }
}

fn encode_entry(seq: u64, record: &WalRecord) -> Result<Vec<u8>> {
    let payload = WalPayload {
        version: WAL_VERSION,
        seq,
        record: record.clone(),
    };
    let payload_bytes = serde_json::to_vec(&payload)
        .map_err(|error| AppError::storage(format!("WAL内容序列化失败：{error}")))?;
    let entry = WalEntry {
        payload,
        crc32: format!("{:08X}", crc32fast::hash(&payload_bytes)),
    };

    serde_json::to_vec(&entry)
        .map_err(|error| AppError::storage(format!("WAL记录序列化失败：{error}")))
}

fn verify_payload_checksum(entry: &WalEntry, line: usize) -> Result<()> {
    let payload_bytes = serde_json::to_vec(&entry.payload)
        .map_err(|error| AppError::corrupt_wal(line, format!("WAL内容序列化失败：{error}")))?;
    verify_checksum(&entry.crc32, &payload_bytes, line)
}

fn verify_wal_checksum(crc32: &str, record: &WalRecord, line: usize) -> Result<()> {
    let record_bytes = serde_json::to_vec(record)
        .map_err(|error| AppError::corrupt_wal(line, format!("WAL操作序列化失败：{error}")))?;
    verify_checksum(crc32, &record_bytes, line)
}

fn verify_checksum(crc32: &str, bytes: &[u8], line: usize) -> Result<()> {
    if crc32.len() != 8 || !crc32.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::corrupt_wal(
            line,
            format!("CRC32格式错误，应为8位十六进制数：{crc32}"),
        ));
    }

    let stored = u32::from_str_radix(crc32, 16).map_err(|_| {
        AppError::corrupt_wal(line, format!("CRC32格式错误，应为8位十六进制数：{crc32}"))
    })?;
    let actual = crc32fast::hash(bytes);
    if stored != actual {
        return Err(AppError::corrupt_wal(
            line,
            format!("CRC32校验失败：文件记录为 {stored:08X}，重新计算为 {actual:08X}"),
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

fn recover_snapshot(path: &Path) -> Result<(Store, u64)> {
    if path.exists() {
        load_snapshot_file(path)
    } else {
        Ok((Store::new(), 0))
    }
}

fn load_snapshot_file(path: &Path) -> Result<(Store, u64)> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_SNAPSHOT_BYTES {
        return Err(snapshot_error(format!(
            "文件为 {} 字节，最大允许 {MAX_SNAPSHOT_BYTES} 字节",
            metadata.len()
        )));
    }

    let bytes = fs::read(path)?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Err(snapshot_error("文件不能为空"));
    }
    let snapshot: SnapshotFile = serde_json::from_slice(&bytes)
        .map_err(|error| snapshot_error(format!("JSON格式错误：{error}")))?;
    if snapshot.payload.version != SNAPSHOT_VERSION {
        return Err(snapshot_error(format!(
            "不支持的快照版本：{}",
            snapshot.payload.version
        )));
    }

    let payload_bytes = serde_json::to_vec(&snapshot.payload)
        .map_err(|error| snapshot_error(format!("内容序列化失败：{error}")))?;
    verify_snapshot_checksum(&snapshot.crc32, &payload_bytes)?;

    if snapshot.payload.last_seq == 0 && !snapshot.payload.entries.is_empty() {
        return Err(snapshot_error("非空快照的last_seq不能为0"));
    }

    let mut store = Store::new();
    let mut previous_key: Option<&str> = None;
    for entry in &snapshot.payload.entries {
        validate_key(&entry.key).map_err(|error| snapshot_error(error.client_message()))?;
        validate_value(&entry.value).map_err(|error| snapshot_error(error.client_message()))?;
        if previous_key.is_some_and(|previous| previous >= entry.key.as_str()) {
            return Err(snapshot_error("快照中的键必须严格按字典序排列且不能重复"));
        }
        previous_key = Some(&entry.key);
        store.set_validated(entry.key.clone(), entry.value.clone());
    }

    Ok((store, snapshot.payload.last_seq))
}

fn encode_snapshot(payload: &SnapshotPayload) -> Result<Vec<u8>> {
    let payload_bytes = serde_json::to_vec(payload)
        .map_err(|error| AppError::storage(format!("快照内容序列化失败：{error}")))?;
    let snapshot = SnapshotFile {
        payload: payload.clone(),
        crc32: format!("{:08X}", crc32fast::hash(&payload_bytes)),
    };
    serde_json::to_vec(&snapshot)
        .map_err(|error| AppError::storage(format!("快照文件序列化失败：{error}")))
}

fn verify_snapshot_checksum(crc32: &str, bytes: &[u8]) -> Result<()> {
    if crc32.len() != 8 || !crc32.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(snapshot_error(format!(
            "CRC32格式错误，应为8位十六进制数：{crc32}"
        )));
    }
    let stored =
        u32::from_str_radix(crc32, 16).map_err(|_| snapshot_error("CRC32不是有效的十六进制数"))?;
    let actual = crc32fast::hash(bytes);
    if stored != actual {
        return Err(snapshot_error(format!(
            "CRC32校验失败：文件记录为 {stored:08X}，重新计算为 {actual:08X}"
        )));
    }
    Ok(())
}

fn restore_snapshot_backup(snapshot_path: &Path) -> Result<()> {
    let backup_path = path_with_suffix(snapshot_path, ".bak");
    if !snapshot_path.exists() && backup_path.exists() {
        fs::rename(backup_path, snapshot_path)?;
    }
    Ok(())
}

fn publish_snapshot(temp_path: &Path, snapshot_path: &Path, backup_path: &Path) -> Result<()> {
    let had_snapshot = snapshot_path.exists();
    if backup_path.exists() {
        fs::remove_file(backup_path)?;
    }
    if had_snapshot {
        fs::rename(snapshot_path, backup_path)?;
    }

    if let Err(publish_error) = fs::rename(temp_path, snapshot_path) {
        if had_snapshot && let Err(restore_error) = fs::rename(backup_path, snapshot_path) {
            return Err(AppError::storage(format!(
                "发布新快照失败：{publish_error}；恢复旧快照也失败：{restore_error}"
            )));
        }
        return Err(AppError::Io(publish_error));
    }
    Ok(())
}

fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

fn snapshot_error(message: impl Into<String>) -> AppError {
    AppError::storage(format!("Snapshot损坏：{}", message.into()))
}
