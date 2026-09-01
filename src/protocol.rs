//! Data types for the JSON Lines protocol.

use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;
use crate::error::{AppError, ErrorCode, Result};
use std::io::{self, BufRead};

/// Maximum request or response payload size, excluding the trailing LF.
pub const MAX_FRAME_BYTES: usize = 65_536;
/// Maximum key size in UTF-8 bytes.
pub const MAX_KEY_BYTES: usize = 256;
/// Maximum value size in UTF-8 bytes.
pub const MAX_VALUE_BYTES: usize = 16 * 1024;

/// 从json之中读取到的单个frame
#[derive(Debug, PartialEq, Eq)]
pub enum Frame {
    /// 完整的frame 包含LF terminator
    Line(Vec<u8>),

    /// 一个过大的frame
    TooLarge,

    /// 没有读取到frame
    Eof,

    /// 一个不完整的frame
    Incomplete,
}

/// 读取frame
pub fn read_frame<R: BufRead>(reader: &mut R) -> io::Result<Frame> {
    let mut frame = Vec::with_capacity(MAX_FRAME_BYTES + 1);
    let mut oversized = false;

    loop {
        let (buffer_len, newline) = {
            let buffer = reader.fill_buf()?;
            if buffer.is_empty() {
                return Ok(if frame.is_empty() {
                    Frame::Eof
                } else {
                    Frame::Incomplete
                });
            }

            let mut newline = None;
            for (index, byte) in buffer.iter().enumerate() {
                if *byte == b'\n' {
                    newline = Some(index + 1);
                    break;
                }

                if frame.len() < MAX_FRAME_BYTES + 1 {
                    frame.push(*byte);
                } else {
                    oversized = true;
                }
            }
            (buffer.len(), newline)
        };

        if let Some(consumed) = newline {
            reader.consume(consumed);
            if oversized {
                return Ok(Frame::TooLarge);
            }

            let payload_len = frame
                .len()
                .saturating_sub(usize::from(frame.last() == Some(&b'\r')));
            if payload_len > MAX_FRAME_BYTES {
                return Ok(Frame::TooLarge);
            }
            frame.push(b'\n');
            return Ok(Frame::Line(frame));
        }
        reader.consume(buffer_len);
    }
}

/// A command accepted by the wire protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "lowercase", deny_unknown_fields)]
pub enum Request {
    Set { key: String, value: String },
    Get { key: String },
    Delete { key: String },
    Keys,
    Status,
    Ping,
    Quit,
}

impl Request {
    /// 在反序列化或者文本传递之后用于验证
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Set { key, value } => {
                crate::storage::validate_key(key)?;
                crate::storage::validate_value(value)
            }
            Self::Get { key } | Self::Delete { key } => crate::storage::validate_key(key),
            Self::Keys | Self::Status | Self::Ping | Self::Quit => Ok(()),
        }
    }
}

/// Data carried by a successful response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum ResponseData {
    Set { replaced: bool },
    Get { value: String },
    Delete { deleted: bool },
    Keys { keys: Vec<String>, count: usize },
    Status { count: usize },
    Ping,
    Quit,
}

/// Details carried by a failed response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

/// One response corresponding to one request.
///
/// Constructors keep the two wire shapes mutually exclusive: successful
/// responses contain `data`, while failed responses contain `error`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<ResponseData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl Response {
    pub fn success(data: ResponseData) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn failure(error: ErrorBody) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(error),
        }
    }

    pub fn error(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::failure(ErrorBody {
            code,
            message: message.into(),
        })
    }

    pub fn from_error(error: &AppError) -> Self {
        Self::error(error.code(), error.client_message())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serializes_to_the_frozen_command_shape() {
        let set = Request::Set {
            key: "name".to_owned(),
            value: "Alice".to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&set).expect("request serializes"),
            r#"{"cmd":"set","key":"name","value":"Alice"}"#
        );
        assert_eq!(
            serde_json::to_string(&Request::Keys).expect("request serializes"),
            r#"{"cmd":"keys"}"#
        );
    }

    #[test]
    fn response_serializes_to_the_frozen_success_and_failure_shapes() {
        let success = Response::success(ResponseData::Set { replaced: false });
        assert_eq!(
            serde_json::to_string(&success).expect("response serializes"),
            r#"{"ok":true,"data":{"kind":"set","replaced":false}}"#
        );

        let failure = Response::error(ErrorCode::NotFound, "missing key");
        assert_eq!(
            serde_json::to_string(&failure).expect("response serializes"),
            r#"{"ok":false,"error":{"code":"NOT_FOUND","message":"missing key"}}"#
        );
    }
}
