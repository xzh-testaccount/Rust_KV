//! Shared library for the key-value store server and command-line client.

pub mod client;
pub mod error;
pub mod persistence;
pub mod protocol;
pub mod server;
pub mod storage;

pub use persistence::PersistentStore;
