//! TCP service-layer entry points.

use std::net::SocketAddr;
use std::path::PathBuf;

use crate::error::{AppError, Result};

pub const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:7878";
pub const DEFAULT_WAL_PATH: &str = "data/kv.wal";

/// Configuration passed to the future TCP listener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    pub wal_path: PathBuf,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND_ADDRESS
                .parse()
                .expect("the built-in bind address must be valid"),
            wal_path: PathBuf::from(DEFAULT_WAL_PATH),
        }
    }
}

/// Starts the server once the network and persistence implementations exist.
pub fn run(_config: ServerConfig) -> Result<()> {
    Err(AppError::NotImplemented("TCP server loop"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_architecture_contract() {
        let config = ServerConfig::default();
        assert_eq!(config.bind.to_string(), DEFAULT_BIND_ADDRESS);
        assert_eq!(config.wal_path, PathBuf::from(DEFAULT_WAL_PATH));
    }
}
