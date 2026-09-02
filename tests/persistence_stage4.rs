use std::fs;

use rust_kv_store::{
    error::{AppError, ErrorCode},
    persistence::PersistentStore,
    storage::SetOutcome,
};
use tempfile::tempdir;

fn checked_line(record: &[u8]) -> Vec<u8> {
    let record = std::str::from_utf8(record).unwrap();
    let crc32 = crc32fast::hash(record.as_bytes());
    let mut line = format!(r#"{{"record":{record},"crc32":"{crc32:08X}"}}"#).into_bytes();
    line.push(b'\n');
    line
}

fn checked_current_line(seq: u64, record: &[u8]) -> Vec<u8> {
    let record = std::str::from_utf8(record).unwrap();
    let payload = format!(r#"{{"version":1,"seq":{seq},"record":{record}}}"#);
    let crc32 = crc32fast::hash(payload.as_bytes());
    let mut line = format!(r#"{{"payload":{payload},"crc32":"{crc32:08X}"}}"#).into_bytes();
    line.push(b'\n');
    line
}

fn assert_corrupt(error: AppError, expected_line: usize, message: &str) {
    assert_eq!(error.code(), ErrorCode::StorageError);
    match error {
        AppError::CorruptWal { line, reason } => {
            assert_eq!(line, expected_line);
            assert!(reason.contains(message), "实际错误：{reason}");
        }
        other => panic!("期望WAL损坏错误，实际为：{other}"),
    }
}

#[test]
fn first_open_creates_directories_and_an_empty_wal() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("nested").join("data").join("kv.wal");

    let store = PersistentStore::open(&wal_path).unwrap();

    assert!(wal_path.is_file());
    assert!(store.is_empty());
    assert_eq!(store.wal_path(), wal_path);
    assert_eq!(store.stats().wal_records, 0);
    assert_eq!(store.stats().wal_bytes, 0);
    assert!(store.stats().writable);
}

#[test]
fn restart_recovers_create_replace_and_delete_in_order() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("kv.wal");

    {
        let mut store = PersistentStore::open(&wal_path).unwrap();
        assert_eq!(
            store.set("course".into(), "Rust".into()).unwrap(),
            SetOutcome::Created
        );
        assert_eq!(
            store.set("course".into(), "Advanced Rust".into()).unwrap(),
            SetOutcome::Replaced {
                previous: "Rust".to_owned(),
            }
        );
        store.set("temporary".into(), "value".into()).unwrap();
        assert_eq!(store.delete("temporary").unwrap(), "value");
        assert_eq!(store.stats().wal_records, 4);
    }

    {
        let mut store = PersistentStore::open(&wal_path).unwrap();
        assert_eq!(store.get("course").unwrap(), "Advanced Rust");
        assert_eq!(store.keys(), vec!["course"]);
        assert_eq!(store.stats().wal_records, 4);
        assert_eq!(
            store.get("temporary").unwrap_err().code(),
            ErrorCode::NotFound
        );

        store.set("name".into(), "Alice".into()).unwrap();
        assert_eq!(store.stats().wal_records, 5);
    }

    let store = PersistentStore::open(&wal_path).unwrap();
    assert_eq!(store.get("course").unwrap(), "Advanced Rust");
    assert_eq!(store.get("name").unwrap(), "Alice");
    assert_eq!(store.stats().wal_records, 5);
}

#[test]
fn successful_mutations_are_complete_json_lines() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("kv.wal");

    {
        let mut store = PersistentStore::open(&wal_path).unwrap();
        store.set("course".into(), "Rust".into()).unwrap();
        store.delete("course").unwrap();
    }

    let bytes = fs::read(&wal_path).unwrap();
    assert_eq!(bytes.last(), Some(&b'\n'));
    let lines = bytes.split(|byte| *byte == b'\n').collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);
    assert!(lines.last().unwrap().is_empty());

    let expected_set = checked_current_line(1, br#"{"op":"set","key":"course","value":"Rust"}"#);
    let expected_delete = checked_current_line(2, br#"{"op":"delete","key":"course"}"#);
    assert_eq!(lines[0], &expected_set[..expected_set.len() - 1]);
    assert_eq!(lines[1], &expected_delete[..expected_delete.len() - 1]);
}

#[test]
fn rejected_mutations_do_not_change_memory_or_wal() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("kv.wal");
    let mut store = PersistentStore::open(&wal_path).unwrap();

    assert_eq!(
        store.set("".into(), "value".into()).unwrap_err().code(),
        ErrorCode::InvalidKey
    );
    assert_eq!(
        store.delete("missing").unwrap_err().code(),
        ErrorCode::NotFound
    );
    assert!(store.is_empty());
    assert_eq!(store.stats().wal_records, 0);
    assert_eq!(store.stats().wal_bytes, 0);
    assert!(fs::read(&wal_path).unwrap().is_empty());
}

#[test]
fn truncated_record_is_reported_without_overwriting_the_file() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("kv.wal");
    let original = br#"{"op":"set","key":"course","value":"Rust"}"#;
    fs::write(&wal_path, original).unwrap();

    let error = PersistentStore::open(&wal_path).unwrap_err();

    assert_corrupt(error, 1, "缺少结尾LF");
    assert_eq!(fs::read(&wal_path).unwrap(), original);
}

#[test]
fn malformed_json_and_unknown_fields_are_rejected() {
    let temp = tempdir().unwrap();
    let malformed_path = temp.path().join("malformed.wal");
    fs::write(&malformed_path, b"{not-json}\n").unwrap();
    assert_corrupt(
        PersistentStore::open(&malformed_path).unwrap_err(),
        1,
        "JSON格式错误",
    );

    let unknown_path = temp.path().join("unknown.wal");
    fs::write(
        &unknown_path,
        checked_line(br#"{"op":"set","key":"course","value":"Rust","extra":true}"#),
    )
    .unwrap();
    assert_corrupt(
        PersistentStore::open(&unknown_path).unwrap_err(),
        1,
        "JSON格式错误",
    );
}

#[test]
fn unknown_operation_and_invalid_data_are_rejected() {
    let temp = tempdir().unwrap();
    let operation_path = temp.path().join("operation.wal");
    fs::write(
        &operation_path,
        checked_line(br#"{"op":"update","key":"course","value":"Rust"}"#),
    )
    .unwrap();
    assert_corrupt(
        PersistentStore::open(&operation_path).unwrap_err(),
        1,
        "JSON格式错误",
    );

    let invalid_key_path = temp.path().join("invalid-key.wal");
    fs::write(
        &invalid_key_path,
        checked_line(br#"{"op":"set","key":"two words","value":"Rust"}"#),
    )
    .unwrap();
    assert_corrupt(
        PersistentStore::open(&invalid_key_path).unwrap_err(),
        1,
        "键不能包含空白字符",
    );
}

#[test]
fn deleting_a_missing_key_in_wal_is_corruption() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("kv.wal");
    fs::write(
        &wal_path,
        checked_line(br#"{"op":"delete","key":"missing"}"#),
    )
    .unwrap();

    assert_corrupt(
        PersistentStore::open(&wal_path).unwrap_err(),
        1,
        "删除的键不存在",
    );
}

#[test]
fn empty_and_oversized_records_are_rejected() {
    let temp = tempdir().unwrap();
    let empty_path = temp.path().join("empty-record.wal");
    fs::write(&empty_path, b"\n").unwrap();
    assert_corrupt(
        PersistentStore::open(&empty_path).unwrap_err(),
        1,
        "记录不能为空",
    );

    let oversized_path = temp.path().join("oversized.wal");
    let oversized = format!(
        "{{\"op\":\"set\",\"key\":\"key\",\"value\":\"{}\"}}\n",
        "v".repeat(65_536)
    );
    fs::write(&oversized_path, oversized).unwrap();
    assert_corrupt(
        PersistentStore::open(&oversized_path).unwrap_err(),
        1,
        "最大允许",
    );
}

#[test]
fn tampered_data_fails_crc32_without_overwriting_the_file() {
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("kv.wal");

    {
        let mut store = PersistentStore::open(&wal_path).unwrap();
        store.set("course".into(), "Rust".into()).unwrap();
    }

    let original = fs::read_to_string(&wal_path).unwrap();
    let tampered = original.replace("\"Rust\"", "\"Dust\"");
    assert_ne!(tampered, original);
    fs::write(&wal_path, &tampered).unwrap();

    assert_corrupt(
        PersistentStore::open(&wal_path).unwrap_err(),
        1,
        "CRC32校验失败",
    );
    assert_eq!(fs::read_to_string(&wal_path).unwrap(), tampered);
}

#[test]
fn missing_and_malformed_crc32_are_rejected() {
    let temp = tempdir().unwrap();
    let missing_path = temp.path().join("missing-crc.wal");
    fs::write(
        &missing_path,
        b"{\"record\":{\"op\":\"set\",\"key\":\"course\",\"value\":\"Rust\"}}\n",
    )
    .unwrap();
    assert_corrupt(
        PersistentStore::open(&missing_path).unwrap_err(),
        1,
        "crc32",
    );

    let malformed_path = temp.path().join("malformed-crc.wal");
    fs::write(
        &malformed_path,
        b"{\"record\":{\"op\":\"set\",\"key\":\"course\",\"value\":\"Rust\"},\"crc32\":\"NOT-CRC\"}\n",
    )
    .unwrap();
    assert_corrupt(
        PersistentStore::open(&malformed_path).unwrap_err(),
        1,
        "CRC32格式错误",
    );
}

#[test]
fn crc32_uses_the_standard_ieee_test_vector() {
    assert_eq!(crc32fast::hash(b"123456789"), 0xCBF4_3926);
}
