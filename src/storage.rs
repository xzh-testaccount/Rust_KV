//! 基于内存的有序键值存储。
//!
//! 所有入口使用同一套键值规则，网络层和持久化层可以直接复用。

use std::collections::BTreeMap;

use crate::error::{AppError, ErrorCode, Result};
pub use crate::protocol::{MAX_KEY_BYTES, MAX_VALUE_BYTES};

/// 写入结果，用于区分新增和覆盖。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetOutcome {
    Created,
    Replaced { previous: String },
}

impl SetOutcome {
    /// 是否覆盖了旧值。
    pub fn replaced(&self) -> bool {
        matches!(self, Self::Replaced { .. })
    }
}

/// 内存存储统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreStats {
    pub entries: usize,
    pub key_bytes: usize,
    pub value_bytes: usize,
}

/// 有序键值存储。
#[derive(Debug, Default)]
pub struct Store {
    entries: BTreeMap<String, String>,
}

impl Store {
    /// 创建空存储。
    pub fn new() -> Self {
        Self::default()
    }

    /// 写入键值，并说明是新增还是覆盖。
    pub fn set<K, V>(&mut self, key: K, value: V) -> Result<SetOutcome>
    where
        K: Into<String>,
        V: Into<String>,
    {
        let key = key.into();
        let value = value.into();
        validate_key(&key)?;
        validate_value(&value)?;

        Ok(self.set_validated(key, value))
    }

    /// 查询键对应的值。
    pub fn get<K>(&self, key: K) -> Result<&str>
    where
        K: AsRef<str>,
    {
        let key = key.as_ref();
        validate_key(key)?;
        self.entries
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| missing_key(key))
    }

    /// 删除键并返回旧值。
    pub fn delete<K>(&mut self, key: K) -> Result<String>
    where
        K: AsRef<str>,
    {
        let key = key.as_ref();
        validate_key(key)?;
        self.entries.remove(key).ok_or_else(|| missing_key(key))
    }

    /// 按字典序返回所有键。
    pub fn keys(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    /// 返回键值数量。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 判断存储是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 统计键值数量和UTF-8字节数。
    pub fn stats(&self) -> StoreStats {
        StoreStats {
            entries: self.entries.len(),
            key_bytes: self.entries.keys().map(String::len).sum(),
            value_bytes: self.entries.values().map(String::len).sum(),
        }
    }

    /// 写入已经校验过的键值。
    pub(crate) fn set_validated(&mut self, key: String, value: String) -> SetOutcome {
        match self.entries.insert(key, value) {
            Some(previous) => SetOutcome::Replaced { previous },
            None => SetOutcome::Created,
        }
    }

    /// 删除已经校验过的键。
    pub(crate) fn delete_validated(&mut self, key: &str) -> Option<String> {
        self.entries.remove(key)
    }

    /// 生成快照时按键顺序复制当前数据。
    pub(crate) fn snapshot_entries(&self) -> Vec<(String, String)> {
        self.entries
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }
}

/// 校验键，供后续WAL恢复复用。
pub(crate) fn validate_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(AppError::protocol(ErrorCode::InvalidKey, "键不能为空"));
    }
    if key.len() > MAX_KEY_BYTES {
        return Err(AppError::protocol(
            ErrorCode::InvalidKey,
            format!("键长度为 {} 字节，最大允许 {MAX_KEY_BYTES} 字节", key.len()),
        ));
    }
    if key.chars().any(char::is_whitespace) {
        return Err(AppError::protocol(
            ErrorCode::InvalidKey,
            "键不能包含空白字符",
        ));
    }
    if key.chars().any(char::is_control) {
        return Err(AppError::protocol(
            ErrorCode::InvalidKey,
            "键不能包含控制字符",
        ));
    }
    Ok(())
}

/// 校验值，供后续WAL恢复复用。
pub(crate) fn validate_value(value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(AppError::protocol(ErrorCode::InvalidValue, "值不能为空"));
    }
    if value.len() > MAX_VALUE_BYTES {
        return Err(AppError::protocol(
            ErrorCode::InvalidValue,
            format!(
                "值长度为 {} 字节，最大允许 {MAX_VALUE_BYTES} 字节",
                value.len()
            ),
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(AppError::protocol(
            ErrorCode::InvalidValue,
            "值不能包含控制字符",
        ));
    }
    Ok(())
}

fn missing_key(key: &str) -> AppError {
    AppError::protocol(ErrorCode::NotFound, format!("键不存在：{key}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_error_code<T>(result: Result<T>, expected: ErrorCode) {
        match result {
            Ok(_) => panic!("本次操作应该失败"),
            Err(error) => assert_eq!(error.code(), expected),
        }
    }

    #[test]
    fn new_store_is_empty() {
        let store = Store::new();

        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
        assert_eq!(
            store.stats(),
            StoreStats {
                entries: 0,
                key_bytes: 0,
                value_bytes: 0,
            }
        );
    }

    #[test]
    fn set_reports_created_and_replaced() {
        let mut store = Store::new();

        let created = store.set("course", "Rust").unwrap();
        assert_eq!(created, SetOutcome::Created);
        assert!(!created.replaced());

        let replaced = store.set("course", "Advanced Rust").unwrap();
        assert_eq!(
            replaced,
            SetOutcome::Replaced {
                previous: "Rust".to_owned()
            }
        );
        assert!(replaced.replaced());
        assert_eq!(store.get("course").unwrap(), "Advanced Rust");
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn get_and_delete_missing_key_return_not_found() {
        let mut store = Store::new();

        assert_error_code(store.get("missing"), ErrorCode::NotFound);
        assert_error_code(store.delete("missing"), ErrorCode::NotFound);
    }

    #[test]
    fn delete_returns_the_old_value() {
        let mut store = Store::new();
        store.set("name", "Alice").unwrap();

        assert_eq!(store.delete("name").unwrap(), "Alice");
        assert!(store.is_empty());
        assert_error_code(store.get("name"), ErrorCode::NotFound);
    }

    #[test]
    fn keys_are_returned_in_dictionary_order() {
        let mut store = Store::new();
        store.set("zeta", "last").unwrap();
        store.set("alpha", "first").unwrap();
        store.set("middle", "between").unwrap();

        assert_eq!(store.keys(), vec!["alpha", "middle", "zeta"]);
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn invalid_keys_are_rejected_without_changing_data() {
        let mut store = Store::new();

        assert_error_code(store.set("", "value"), ErrorCode::InvalidKey);
        assert_error_code(store.set("two words", "value"), ErrorCode::InvalidKey);
        assert_error_code(store.set("line\nbreak", "value"), ErrorCode::InvalidKey);
        assert_error_code(
            store.set("k".repeat(MAX_KEY_BYTES + 1), "value"),
            ErrorCode::InvalidKey,
        );
        assert!(store.is_empty());
    }

    #[test]
    fn key_limit_uses_utf8_bytes() {
        let mut store = Store::new();
        let valid = "键".repeat(MAX_KEY_BYTES / "键".len());
        let invalid = format!("{valid}键");

        assert!(valid.len() <= MAX_KEY_BYTES);
        assert!(store.set(valid, "value").is_ok());
        assert_error_code(store.set(invalid, "value"), ErrorCode::InvalidKey);
    }

    #[test]
    fn invalid_values_are_rejected_without_changing_data() {
        let mut store = Store::new();

        assert_error_code(store.set("empty", ""), ErrorCode::InvalidValue);
        assert_error_code(store.set("control", "line\nbreak"), ErrorCode::InvalidValue);
        assert_error_code(
            store.set("large", "v".repeat(MAX_VALUE_BYTES + 1)),
            ErrorCode::InvalidValue,
        );
        assert!(store.is_empty());
    }

    #[test]
    fn value_can_contain_spaces_and_reach_the_limit() {
        let mut store = Store::new();
        let maximum_value = "v".repeat(MAX_VALUE_BYTES);

        assert!(store.set("title", "Advanced Rust").is_ok());
        assert!(store.set("maximum", maximum_value).is_ok());
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn stats_include_utf8_byte_counts() {
        let mut store = Store::new();
        store.set("课程", "Rust语言").unwrap();
        store.set("name", "Alice").unwrap();

        assert_eq!(
            store.stats(),
            StoreStats {
                entries: 2,
                key_bytes: "课程".len() + "name".len(),
                value_bytes: "Rust语言".len() + "Alice".len(),
            }
        );
    }
}
