use std::{ffi::OsString, fs, path::Path};

use rust_kv_store::{
    error::AppError, persistence::PersistentStore,
    persistence_basic::PersistentStore as BasicPersistentStore, storage::SetOutcome,
};
use tempfile::tempdir;

fn legacy_line(record: &str) -> Vec<u8> {
    let crc32 = crc32fast::hash(record.as_bytes());
    format!(
        r#"{{"record":{record},"crc32":"{crc32:08X}"}}
"#
    )
    .into_bytes()
}

fn current_line(seq: u64, record: &str) -> Vec<u8> {
    let payload = format!(r#"{{"version":1,"seq":{seq},"record":{record}}}"#);
    let crc32 = crc32fast::hash(payload.as_bytes());
    format!(
        r#"{{"payload":{payload},"crc32":"{crc32:08X}"}}
"#
    )
    .into_bytes()
}

fn with_suffix(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut value: OsString = path.as_os_str().to_owned();
    value.push(suffix);
    value.into()
}

fn assert_wal_reason(error: AppError, expected_line: usize, text: &str) {
    match error {
        AppError::CorruptWal { line, reason } => {
            assert_eq!(line, expected_line);
            assert!(reason.contains(text), "实际错误：{reason}");
        }
        other => panic!("期望WAL损坏错误，实际为：{other}"),
    }
}

#[test]
fn basic_store_remains_available_for_control_experiments() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("basic.wal");

    {
        let mut store = BasicPersistentStore::open(&wal_path).unwrap();
        store.set("course".into(), "Rust".into()).unwrap();
        assert_eq!(store.get("course").unwrap(), "Rust");
    }

    let store = BasicPersistentStore::open(&wal_path).unwrap();
    assert_eq!(store.get("course").unwrap(), "Rust");
    assert_eq!(store.stats().wal_records, 1);
}

#[test]
fn compact_creates_snapshot_and_restart_keeps_final_state() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("kv.wal");
    let snapshot_path = wal_path.with_extension("snapshot");

    {
        let mut store = PersistentStore::open(&wal_path).unwrap();
        store.set("zeta".into(), "one".into()).unwrap();
        store.set("alpha".into(), "first".into()).unwrap();
        store.set("zeta".into(), "two".into()).unwrap();
        store.set("temporary".into(), "gone".into()).unwrap();
        store.delete("temporary").unwrap();

        let result = store.compact().unwrap();
        assert_eq!(result.records_before, 5);
        assert!(result.wal_bytes_before > 0);
        assert_eq!(result.records_after, 0);
        assert_eq!(result.wal_bytes_after, 0);
        assert_eq!(result.snapshot_entries, 2);
        assert_eq!(result.last_seq, 5);
        assert_eq!(
            result.snapshot_bytes,
            fs::metadata(&snapshot_path).unwrap().len()
        );
        assert_eq!(store.stats().wal_records, 0);
        assert_eq!(store.stats().wal_bytes, 0);
        assert!(fs::read(&wal_path).unwrap().is_empty());
    }

    let store = PersistentStore::open(&wal_path).unwrap();
    assert_eq!(store.keys(), vec!["alpha", "zeta"]);
    assert_eq!(store.get("alpha").unwrap(), "first");
    assert_eq!(store.get("zeta").unwrap(), "two");
    assert_eq!(store.last_sequence(), 5);
    assert_eq!(store.stats().wal_records, 0);
}

#[test]
fn writes_after_compaction_continue_sequence_and_recover() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("kv.wal");

    {
        let mut store = PersistentStore::open(&wal_path).unwrap();
        store.set("course".into(), "Rust".into()).unwrap();
        store.compact().unwrap();
        store.set("course".into(), "Advanced Rust".into()).unwrap();
        store.set("name".into(), "Alice".into()).unwrap();
        assert_eq!(store.last_sequence(), 3);
        assert_eq!(store.stats().wal_records, 2);
    }

    let store = PersistentStore::open(&wal_path).unwrap();
    assert_eq!(store.get("course").unwrap(), "Advanced Rust");
    assert_eq!(store.get("name").unwrap(), "Alice");
    assert_eq!(store.last_sequence(), 3);
}

#[test]
fn snapshot_with_untruncated_wal_does_not_replay_old_changes() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("kv.wal");
    let old_wal;

    {
        let mut store = PersistentStore::open(&wal_path).unwrap();
        store.set("course".into(), "Rust".into()).unwrap();
        store.set("temporary".into(), "value".into()).unwrap();
        store.delete("temporary").unwrap();
        old_wal = fs::read(&wal_path).unwrap();
        store.compact().unwrap();
    }

    // 模拟“快照已发布，但WAL还没来得及截断”时进程退出。
    fs::write(&wal_path, old_wal).unwrap();
    let store = PersistentStore::open(&wal_path).unwrap();
    assert_eq!(store.get("course").unwrap(), "Rust");
    assert_eq!(store.keys(), vec!["course"]);
    assert_eq!(store.last_sequence(), 3);
}

#[test]
fn corrupted_snapshot_is_rejected_without_overwriting_it() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("kv.wal");
    let snapshot_path = wal_path.with_extension("snapshot");

    {
        let mut store = PersistentStore::open(&wal_path).unwrap();
        store.set("course".into(), "Rust".into()).unwrap();
        store.compact().unwrap();
    }

    let original = fs::read_to_string(&snapshot_path).unwrap();
    let damaged = original.replace("\"Rust\"", "\"Dust\"");
    assert_ne!(damaged, original);
    fs::write(&snapshot_path, &damaged).unwrap();

    let error = PersistentStore::open(&wal_path).unwrap_err();
    match error {
        AppError::Storage { message } => {
            assert!(message.contains("Snapshot损坏"));
            assert!(message.contains("CRC32校验失败"));
        }
        other => panic!("期望Snapshot损坏错误，实际为：{other}"),
    }
    assert_eq!(fs::read_to_string(snapshot_path).unwrap(), damaged);
}

#[test]
fn startup_ignores_unpublished_temp_snapshot() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("kv.wal");
    let snapshot_temp = with_suffix(&wal_path.with_extension("snapshot"), ".tmp");

    {
        let mut store = PersistentStore::open(&wal_path).unwrap();
        store.set("course".into(), "Rust".into()).unwrap();
    }
    fs::write(&snapshot_temp, b"unfinished snapshot").unwrap();

    let store = PersistentStore::open(&wal_path).unwrap();
    assert_eq!(store.get("course").unwrap(), "Rust");
    assert_eq!(fs::read(snapshot_temp).unwrap(), b"unfinished snapshot");
}

#[test]
fn startup_restores_snapshot_backup_if_publish_was_interrupted() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("kv.wal");
    let snapshot_path = wal_path.with_extension("snapshot");
    let backup_path = with_suffix(&snapshot_path, ".bak");

    {
        let mut store = PersistentStore::open(&wal_path).unwrap();
        store.set("course".into(), "Rust".into()).unwrap();
        store.compact().unwrap();
    }
    fs::rename(&snapshot_path, &backup_path).unwrap();

    let store = PersistentStore::open(&wal_path).unwrap();
    assert_eq!(store.get("course").unwrap(), "Rust");
    assert!(snapshot_path.is_file());
    assert!(!backup_path.exists());
}

#[test]
fn sequence_gap_is_reported_at_the_exact_line() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("kv.wal");
    let first = current_line(1, r#"{"op":"set","key":"a","value":"one"}"#);
    let third = current_line(3, r#"{"op":"set","key":"b","value":"two"}"#);
    let mut wal = first;
    wal.extend(third);
    fs::write(&wal_path, wal).unwrap();

    assert_wal_reason(
        PersistentStore::open(&wal_path).unwrap_err(),
        2,
        "WAL序号不连续",
    );
}

#[test]
fn legacy_wal_can_upgrade_without_rewriting_old_records() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("kv.wal");
    fs::write(
        &wal_path,
        legacy_line(r#"{"op":"set","key":"course","value":"Rust"}"#),
    )
    .unwrap();

    {
        let mut store = PersistentStore::open(&wal_path).unwrap();
        assert_eq!(store.get("course").unwrap(), "Rust");
        assert_eq!(store.last_sequence(), 1);
        assert_eq!(
            store.set("name".into(), "Alice".into()).unwrap(),
            SetOutcome::Created
        );
        assert_eq!(store.last_sequence(), 2);
    }

    let store = PersistentStore::open(&wal_path).unwrap();
    assert_eq!(store.get("course").unwrap(), "Rust");
    assert_eq!(store.get("name").unwrap(), "Alice");
    assert_eq!(store.stats().wal_records, 2);
    assert_eq!(store.last_sequence(), 2);
}
