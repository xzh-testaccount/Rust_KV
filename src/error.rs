//! Errors shared by the key-value store layers.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Stable error codes used in protocol error responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidUtf8,
    InvalidJson,
    InvalidRequest,
    UnknownCommand,
    MissingArgument,
    ExtraArgument,
    InvalidKey,
    InvalidValue,
    NotFound,
    FrameTooLarge,
    StorageError,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidUtf8 => "INVALID_UTF8",
            Self::InvalidJson => "INVALID_JSON",
            Self::InvalidRequest => "INVALID_REQUEST",
            Self::UnknownCommand => "UNKNOWN_COMMAND",
            Self::MissingArgument => "MISSING_ARGUMENT",
            Self::ExtraArgument => "EXTRA_ARGUMENT",
            Self::InvalidKey => "INVALID_KEY",
            Self::InvalidValue => "INVALID_VALUE",
            Self::NotFound => "NOT_FOUND",
            Self::FrameTooLarge => "FRAME_TOO_LARGE",
            Self::StorageError => "STORAGE_ERROR",
        })
    }
}

/// Application errors retained between the protocol, storage, and I/O layers.
#[derive(Debug)]
pub enum AppError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Protocol { code: ErrorCode, message: String },
    Storage { message: String },
    CorruptWal { line: usize, reason: String },
    NotImplemented(&'static str),
}

impl AppError {
    /// Builds a protocol error with a stable wire code.
    pub fn protocol(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Protocol {
            code,
            message: message.into(),
        }
    }

    /// 创建存储错误。
    pub fn storage(message: impl Into<String>) -> Self {
        Self::Storage {
            message: message.into(),
        }
    }

    /// 创建带行号的WAL损坏错误。
    pub fn corrupt_wal(line: usize, reason: impl Into<String>) -> Self {
        Self::CorruptWal {
            line,
            reason: reason.into(),
        }
    }

    /// Returns the wire code for this error.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Io(_)
            | Self::Storage { .. }
            | Self::CorruptWal { .. }
            | Self::NotImplemented(_) => ErrorCode::StorageError,
            Self::Json(_) => ErrorCode::InvalidJson,
            Self::Protocol { code, .. } => *code,
        }
    }

    /// Returns a message suitable for an error response.
    pub fn client_message(&self) -> String {
        match self {
            Self::Io(_) => "internal storage error".to_owned(),
            Self::Json(error) => error.to_string(),
            Self::Protocol { message, .. } => message.clone(),
            Self::Storage { message } => message.clone(),
            Self::CorruptWal { line, reason } => {
                format!("WAL第 {line} 行损坏：{reason}")
            }
            Self::NotImplemented(message) => (*message).to_owned(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Json(error) => write!(formatter, "JSON error: {error}"),
            Self::Protocol { code, message } => write!(formatter, "{code}: {message}"),
            Self::Storage { message } => write!(formatter, "storage error: {message}"),
            Self::CorruptWal { line, reason } => {
                write!(formatter, "corrupt WAL at line {line}: {reason}")
            }
            Self::NotImplemented(feature) => write!(formatter, "not implemented: {feature}"),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Protocol { .. }
            | Self::Storage { .. }
            | Self::CorruptWal { .. }
            | Self::NotImplemented(_) => None,
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
