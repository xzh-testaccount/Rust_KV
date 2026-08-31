//! Validated, ordered in-memory key-value storage.

use crate::error::{AppError, Result};
use crate::protocol::{MAX_KEY_BYTES, MAX_VALUE_BYTES};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The result of inserting a key/value pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetOutcome {
    pub replaced: bool,
}

/// The result of deleting an existing key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeleteOutcome {
    pub deleted: bool,
}

/// Current storage statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreStatus {
    pub count: usize,
}

/// Ordered in-memory key-value store.
#[derive(Debug, Default)]
pub struct Store {
    entries: BTreeMap<String, String>,
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or overwrites a value after validating both fields.
    pub fn set(&mut self, key: &str, value: &str) -> Result<SetOutcome> {
        validate_key(key)?;
        validate_value(value)?;
        let replaced = self
            .entries
            .insert(key.to_owned(), value.to_owned())
            .is_some();
        Ok(SetOutcome { replaced })
    }

    /// Returns a copy of the value, or a stable not-found error.
    pub fn get(&self, key: &str) -> Result<String> {
        validate_key(key)?;
        self.entries
            .get(key)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("key {key:?} does not exist")))
    }

    /// Removes an existing key, or returns a stable not-found error.
    pub fn delete(&mut self, key: &str) -> Result<DeleteOutcome> {
        validate_key(key)?;
        if self.entries.remove(key).is_some() {
            Ok(DeleteOutcome { deleted: true })
        } else {
            Err(AppError::NotFound(format!("key {key:?} does not exist")))
        }
    }

    /// Returns all keys in lexicographic order.
    pub fn keys(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn contains_key(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    pub fn status(&self) -> StoreStatus {
        StoreStatus { count: self.len() }
    }
}

/// Backwards-compatible name for callers of the original skeleton.
pub type Storage = Store;

pub fn validate_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(AppError::InvalidKey("key must not be empty".to_owned()));
    }
    if key.len() > MAX_KEY_BYTES {
        return Err(AppError::InvalidKey(format!(
            "key must be at most {MAX_KEY_BYTES} UTF-8 bytes"
        )));
    }
    if key
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(AppError::InvalidKey(
            "key must not contain whitespace or control characters".to_owned(),
        ));
    }
    Ok(())
}

pub fn validate_value(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(AppError::InvalidValue("value must not be empty".to_owned()));
    }
    if value.len() > MAX_VALUE_BYTES {
        return Err(AppError::InvalidValue(format!(
            "value must be at most {MAX_VALUE_BYTES} UTF-8 bytes"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(AppError::InvalidValue(
            "value must not contain control characters".to_owned(),
        ));
    }
    Ok(())
}
