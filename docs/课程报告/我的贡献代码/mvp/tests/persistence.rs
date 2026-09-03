//! Persistence tests protect WAL durability and strict startup recovery.

use rust_kv_store::error::{AppError, ErrorCode};
use rust_kv_store::persistence::PersistentStore;
use rust_kv_store::protocol::MAX_FRAME_BYTES;
use tempfile::tempdir;

fn error_code(error: AppError) -> ErrorCode {
    error.code()
}

fn wal_line(mut bytes: Vec<u8>) -> Vec<u8> {
    bytes.push(b'\n');
    bytes
}

#[test]
fn open_creates_missing_parent_and_empty_wal() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("nested").join("store.wal");
    let store = PersistentStore::open(&path).expect("create WAL");
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
    assert!(path.is_file());
    assert_eq!(std::fs::read(&path).expect("read empty WAL"), b"");
}

#[test]
fn set_overwrite_delete_and_restart_recover_final_state() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("store.wal");
    {
        let mut store = PersistentStore::open(&path).expect("open WAL");
        assert!(!store.set("a", "one").expect("insert").replaced);
        assert!(store.set("a", "two").expect("overwrite").replaced);
        assert!(!store.set("b", "three").expect("insert second").replaced);
        assert!(store.delete("b").expect("delete").deleted);
    }
    let store = PersistentStore::open(&path).expect("restore WAL");
    assert_eq!(store.get("a").expect("restored value"), "two");
    assert_eq!(store.keys(), vec!["a".to_owned()]);
    assert_eq!(store.len(), 1);
    assert_eq!(
        error_code(store.get("b").expect_err("deleted value absent")),
        ErrorCode::NotFound
    );
}

#[test]
fn delete_missing_does_not_append_a_record() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("store.wal");
    let mut store = PersistentStore::open(&path).expect("open WAL");
    let before = std::fs::read(&path).expect("read WAL");
    assert_eq!(
        error_code(store.delete("missing").expect_err("missing delete")),
        ErrorCode::NotFound
    );
    assert_eq!(std::fs::read(&path).expect("read unchanged WAL"), before);
}

#[test]
fn every_corrupt_wal_is_rejected_with_line_number_and_unchanged_bytes() {
    let cases: &[(&str, Vec<u8>, &str)] = &[
        ("empty", b"\n".to_vec(), "record is empty"),
        ("invalid JSON", b"not-json\n".to_vec(), "invalid WAL JSON"),
        (
            "unknown op",
            wal_line(br#"{"op":"rename","key":"k"}"#.to_vec()),
            "unknown variant",
        ),
        (
            "missing field",
            wal_line(br#"{"op":"set","key":"k"}"#.to_vec()),
            "missing field",
        ),
        (
            "extra field",
            wal_line(br#"{"op":"set","key":"k","value":"v","extra":1}"#.to_vec()),
            "unknown field",
        ),
        (
            "invalid key",
            wal_line(br#"{"op":"set","key":"","value":"v"}"#.to_vec()),
            "invalid set record",
        ),
        (
            "invalid value",
            wal_line(br#"{"op":"set","key":"k","value":""}"#.to_vec()),
            "invalid set record",
        ),
        (
            "delete missing",
            wal_line(br#"{"op":"delete","key":"missing"}"#.to_vec()),
            "invalid delete record",
        ),
        (
            "invalid UTF-8",
            vec![b'{', 0xff, b'}', b'\n'],
            "invalid WAL JSON",
        ),
        (
            "missing LF",
            br#"{"op":"set","key":"k","value":"v"}"#.to_vec(),
            "not terminated by LF",
        ),
    ];

    for (name, bytes, reason) in cases {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("store.wal");
        std::fs::write(&path, bytes).expect("write corrupt WAL");
        let error = PersistentStore::open(&path).expect_err(name);
        let message = format!("{error}");
        assert_eq!(error.code(), ErrorCode::StorageError, "case: {name}");
        assert!(message.contains("line 1"), "case {name}: {message}");
        assert!(message.contains(reason), "case {name}: {message}");
        assert_eq!(std::fs::read(&path).expect("read original WAL"), *bytes);
    }
}

#[test]
fn later_line_corruption_reports_one_based_line_number() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("store.wal");
    let bytes = br#"{"op":"set","key":"ok","value":"v"}

"#;
    std::fs::write(&path, bytes).expect("write WAL");
    let error = PersistentStore::open(&path).expect_err("second line is empty");
    assert!(format!("{error}").contains("line 2"));
}

#[test]
fn oversized_wal_record_is_rejected_without_rewriting_original_bytes() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("store.wal");
    let mut bytes = format!(
        r#"{{"op":"set","key":"k","value":"{}"}}"#,
        "x".repeat(MAX_FRAME_BYTES)
    )
    .into_bytes();
    bytes.push(b'\n');
    std::fs::write(&path, &bytes).expect("write oversized WAL");

    let error = PersistentStore::open(&path).expect_err("oversized WAL");
    assert_eq!(error.code(), ErrorCode::StorageError);
    let message = format!("{error}");
    assert!(message.contains("line 1"), "{message}");
    assert!(
        message.contains("exceeds 65536-byte payload limit"),
        "{message}"
    );
    assert_eq!(std::fs::read(&path).expect("read original WAL"), bytes);
}
