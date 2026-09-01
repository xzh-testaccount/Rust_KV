//! Command-line client-layer entry points.

use std::net::SocketAddr;

use crate::error::{AppError, Result};
use crate::server::DEFAULT_BIND_ADDRESS;

/// Configuration passed to the future interactive client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    pub server: SocketAddr,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server: DEFAULT_BIND_ADDRESS
                .parse()
                .expect("the built-in server address must be valid"),
        }
    }
}

/// Starts the client once command parsing and TCP transport are implemented.
pub fn run(_config: ClientConfig) -> Result<()> {
    Err(AppError::NotImplemented("interactive TCP client"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_and_server_share_the_default_address() {
        assert_eq!(
            ClientConfig::default().server.to_string(),
            DEFAULT_BIND_ADDRESS
        );
    }
}
