//! 正式持久化入口：默认使用带 Snapshot 和 WAL 压缩的高级实现。

pub use crate::persistence_advanced::{CompactionStats, PersistentStats, PersistentStore};
