//! Ordered, in-memory key-value storage.
//!
//! This module deliberately owns only the in-memory boundary.  Validation,
//! persistence, and protocol concerns belong to higher layers that can be
//! added in later iterations.

use std::collections::BTreeMap;

/// A small ordered key-value store.
#[derive(Debug, Default)]
pub struct Store {
    entries: BTreeMap<String, String>,
}

impl Store {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts `value` under `key` and returns the value previously stored.
    pub fn set<K, V>(&mut self, key: K, value: V) -> Option<String>
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.entries.insert(key.into(), value.into())
    }

    /// Returns the value for `key`, if it exists.
    pub fn get<K>(&self, key: K) -> Option<&str>
    where
        K: AsRef<str>,
    {
        self.entries.get(key.as_ref()).map(String::as_str)
    }

    /// Removes `key` and returns its value, if it exists.
    pub fn delete<K>(&mut self, key: K) -> Option<String>
    where
        K: AsRef<str>,
    {
        self.entries.remove(key.as_ref())
    }

    /// Returns a snapshot of all keys in `BTreeMap` order.
    pub fn keys(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    /// Returns the number of stored entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether the store has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::Store;

    #[test]
    fn keys_are_returned_in_dictionary_order() {
        let mut store = Store::new();
        store.set("zeta", "last");
        store.set("alpha", "first");
        store.set("middle", "between");

        assert_eq!(
            store.keys(),
            vec!["alpha".to_owned(), "middle".to_owned(), "zeta".to_owned()]
        );
    }
}
