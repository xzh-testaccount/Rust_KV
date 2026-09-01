//! Application-wide errors and protocol-facing error codes.

use std::fmt;

/// Stable error codes sent to clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
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
        let value = match self {
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
        };
        formatter.write_str(value)
    }
}

/// Errors shared by the protocol, storage, persistence, server, and client layers.
#[derive(Debug)]
pub enum AppError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidKey(String),
    InvalidValue(String),
    NotFound(String),
    Protocol { code: ErrorCode, message: String },
    Persistence(String),
    Message(String),
}

impl AppError {
    /// Returns the stable code appropriate for a client response.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Io(_) | Self::Persistence(_) | Self::Message(_) => ErrorCode::StorageError,
            Self::Json(_) => ErrorCode::InvalidJson,
            Self::InvalidKey(_) => ErrorCode::InvalidKey,
            Self::InvalidValue(_) => ErrorCode::InvalidValue,
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::Protocol { code, .. } => *code,
        }
    }

    /// Creates an error with a stable protocol code.
    pub fn protocol(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::Protocol {
            code,
            message: message.into(),
        }
    }

    /// Returns the human-readable message that may be shown to a client.
    pub fn client_message(&self) -> String {
        match self {
            Self::Io(_) | Self::Persistence(_) => "internal storage error".to_owned(),
            Self::Json(error) => error.to_string(),
            Self::InvalidKey(message)
            | Self::InvalidValue(message)
            | Self::NotFound(message)
            | Self::Message(message) => message.clone(),
            Self::Protocol { message, .. } => message.clone(),
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Json(error) => write!(formatter, "JSON error: {error}"),
            Self::InvalidKey(message) => write!(formatter, "invalid key: {message}"),
            Self::InvalidValue(message) => write!(formatter, "invalid value: {message}"),
            Self::NotFound(message) => write!(formatter, "not found: {message}"),
            Self::Protocol { code, message } => write!(formatter, "{code}: {message}"),
            Self::Persistence(message) => write!(formatter, "persistence error: {message}"),
            Self::Message(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for AppError {}

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
