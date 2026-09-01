//! Data types for the JSON Lines protocol.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, ErrorCode};

/// Maximum request or response payload size, excluding the trailing LF.
pub const MAX_FRAME_BYTES: usize = 65_536;
/// Maximum key size in UTF-8 bytes.
pub const MAX_KEY_BYTES: usize = 256;
/// Maximum value size in UTF-8 bytes.
pub const MAX_VALUE_BYTES: usize = 16 * 1024;

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
