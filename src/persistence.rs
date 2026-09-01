//! Composition boundary for in-memory storage and future WAL persistence.
//!
//! Opening, appending, recovering, and syncing a WAL are intentionally left
//! for a later iteration.  This module currently carries only the state and
//! accessors needed to introduce those operations without coupling callers to
//! the underlying representation.

use crate::storage::Store;
use std::path::{Path, PathBuf};

/// A store together with the path reserved for its future WAL.
#[derive(Debug, Default)]
pub struct PersistentStore {
    store: Store,
    wal_path: PathBuf,
}

impl PersistentStore {
    /// Creates an empty persistent-store boundary for `wal_path`.
    ///
    /// The path is recorded only; no file is opened or otherwise touched.
    pub fn new(wal_path: impl Into<PathBuf>) -> Self {
        Self {
            store: Store::new(),
            wal_path: wal_path.into(),
        }
    }

    /// Borrows the in-memory store.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Mutably borrows the in-memory store.
    pub fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    /// Returns the path reserved for the future WAL.
    pub fn wal_path(&self) -> &Path {
        &self.wal_path
    }
}
