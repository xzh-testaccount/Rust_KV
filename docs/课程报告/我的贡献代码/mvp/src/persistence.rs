//! Append-only JSON Lines persistence for the key-value store.

use crate::error::{AppError, Result};
use crate::protocol::{Frame, read_frame};
use crate::storage::{DeleteOutcome, SetOutcome, Store, StoreStatus, validate_key, validate_value};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::Path;

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
enum WalRecord {
    Set { key: String, value: String },
    Delete { key: String },
}

/// A key-value store backed by an append-only, durable JSON Lines WAL.
#[derive(Debug)]
pub struct PersistentStore {
    store: Store,
    wal: File,
    writable: bool,
    #[cfg(test)]
    fail_writes_for_test: bool,
}

impl PersistentStore {
    /// Opens or creates a WAL, strictly validates it, and restores its final state.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }

        match File::open(path) {
            Ok(file) => drop(file),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                OpenOptions::new().write(true).create_new(true).open(path)?;
            }
            Err(error) => return Err(error.into()),
        }

        let store = recover_wal(File::open(path)?)?;
        let wal = OpenOptions::new().append(true).read(true).open(path)?;
        Ok(Self {
            store,
            wal,
            writable: true,
            #[cfg(test)]
            fail_writes_for_test: false,
        })
    }

    /// Appends a durable set record before changing memory.
    pub fn set(&mut self, key: &str, value: &str) -> Result<SetOutcome> {
        self.ensure_writable()?;
        validate_key(key)?;
        validate_value(value)?;
        let replaced = self.store.contains_key(key);
        self.append_record(&WalRecord::Set {
            key: key.to_owned(),
            value: value.to_owned(),
        })?;
        self.store.set(key, value)?;
        Ok(SetOutcome { replaced })
    }

    /// Reads a value without requiring the WAL to remain writable.
    pub fn get(&self, key: &str) -> Result<String> {
        self.store.get(key)
    }

    /// Appends a durable delete record before changing memory.
    pub fn delete(&mut self, key: &str) -> Result<DeleteOutcome> {
        self.ensure_writable()?;
        validate_key(key)?;
        if !self.store.contains_key(key) {
            return Err(AppError::NotFound(format!("key {key:?} does not exist")));
        }
        self.append_record(&WalRecord::Delete {
            key: key.to_owned(),
        })?;
        self.store.delete(key)
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

    pub fn status(&self) -> StoreStatus {
        self.store.status()
    }

    fn ensure_writable(&self) -> Result<()> {
        if self.writable {
            Ok(())
        } else {
            Err(AppError::Persistence(
                "persistent store is permanently unwritable after a WAL I/O failure".to_owned(),
            ))
        }
    }

    fn append_record(&mut self, record: &WalRecord) -> Result<()> {
        let mut encoded = serde_json::to_vec(record)?;
        encoded.push(b'\n');

        #[cfg(test)]
        if self.fail_writes_for_test {
            self.writable = false;
            return Err(AppError::Persistence(
                "injected WAL write failure".to_owned(),
            ));
        }

        let result = (|| {
            self.wal.write_all(&encoded)?;
            self.wal.flush()?;
            self.wal.sync_data()?;
            Ok::<(), std::io::Error>(())
        })();
        if let Err(error) = result {
            self.writable = false;
            return Err(AppError::Persistence(format!("WAL write failed: {error}")));
        }
        Ok(())
    }

    #[cfg(test)]
    fn inject_write_failure_for_test(&mut self) {
        self.fail_writes_for_test = true;
    }
}

fn recover_wal(file: File) -> Result<Store> {
    let mut store = Store::new();
    let mut reader = BufReader::new(file);
    let mut line_number = 0usize;

    loop {
        let frame = read_frame(&mut reader)?;
        match frame {
            Frame::Eof => break,
            Frame::Incomplete => {
                line_number += 1;
                return Err(corrupt(line_number, "record is not terminated by LF"));
            }
            Frame::TooLarge => {
                line_number += 1;
                return Err(corrupt(
                    line_number,
                    "record exceeds 65536-byte payload limit",
                ));
            }
            Frame::Line(mut encoded) => {
                line_number += 1;
                encoded.pop();
                if encoded.last() == Some(&b'\r') {
                    encoded.pop();
                }
                if encoded.is_empty() {
                    return Err(corrupt(line_number, "record is empty"));
                }

                let record: WalRecord = serde_json::from_slice(&encoded)
                    .map_err(|error| corrupt(line_number, format!("invalid WAL JSON: {error}")))?;
                match record {
                    WalRecord::Set { key, value } => {
                        store.set(&key, &value).map_err(|error| {
                            corrupt(line_number, format!("invalid set record: {error}"))
                        })?;
                    }
                    WalRecord::Delete { key } => {
                        validate_key(&key).map_err(|error| {
                            corrupt(line_number, format!("invalid delete record: {error}"))
                        })?;
                        store.delete(&key).map_err(|error| {
                            corrupt(line_number, format!("invalid delete record: {error}"))
                        })?;
                    }
                }
            }
        }
    }
    Ok(store)
}

fn corrupt(line_number: usize, message: impl Into<String>) -> AppError {
    AppError::Persistence(format!("WAL line {line_number}: {}", message.into()))
}

#[cfg(test)]
mod tests {
    use super::PersistentStore;
    use crate::error::ErrorCode;
    use tempfile::tempdir;

    #[test]
    fn injected_write_failure_preserves_memory_and_poison_writes() {
        let directory = tempdir().expect("temporary directory");
        let mut store =
            PersistentStore::open(directory.path().join("store.wal")).expect("open empty WAL");
        store.set("existing", "confirmed").expect("confirmed write");
        let wal_before_failure =
            std::fs::read(directory.path().join("store.wal")).expect("read WAL before failure");
        store.inject_write_failure_for_test();

        let error = store.set("key", "value").expect_err("injected failure");
        assert_eq!(error.code(), ErrorCode::StorageError);
        assert_eq!(
            store.get("existing").expect("confirmed value remains"),
            "confirmed"
        );
        assert_eq!(store.keys(), vec!["existing".to_owned()]);
        assert_eq!(store.status().count, 1);
        assert_eq!(
            std::fs::read(directory.path().join("store.wal")).expect("read unchanged WAL"),
            wal_before_failure
        );
        assert_eq!(
            store
                .set("key", "value")
                .expect_err("later writes remain rejected")
                .code(),
            ErrorCode::StorageError
        );
        assert_eq!(
            store
                .delete("existing")
                .expect_err("later deletes remain rejected")
                .code(),
            ErrorCode::StorageError
        );
        assert_eq!(
            store.get("existing").expect("reads remain available"),
            "confirmed"
        );
        assert_eq!(store.keys(), vec!["existing".to_owned()]);
        assert_eq!(store.status().count, 1);
        assert_eq!(
            std::fs::read(directory.path().join("store.wal")).expect("read final WAL"),
            wal_before_failure
        );
    }
}
