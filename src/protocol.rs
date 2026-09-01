//! 包含JSON Lines协议的实现以及客户端-服务端交互命令协议的实现

use crate::error::{AppError, ErrorCode, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead};

/// 包含LF的情况下, 请求响应的最大负载的字节数
pub const MAX_FRAME_BYTES: usize = 65_536;
/// 使用utf-8编码时, key的最大字节数
pub const MAX_KEY_BYTES: usize = 256;
/// 使用utf-8编码时, value的最大字节数
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

/// 客户端和服务端交互采用的command格式
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

/// 返回数据之中的响应数据
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

/// 响应错误时的响应数据
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

/// 一个响应对应着一个请求
///
/// 使得成功的响应包含所要求的数据, 失败的响应包含错误信息
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

/// 解析一个完整的JSON Lines 请求
pub fn parse_request_line(line: &str) -> Result<Request> {
    let payload = strip_line_terminator(line)?;
    ensure_frame_size(payload.len())?;
    if payload.is_empty() {
        return Err(AppError::protocol(
            ErrorCode::InvalidRequest,
            "request frame is empty",
        ));
    }

    let value: serde_json::Value = decode_json(payload)?;
    validate_request_fields(&value)?;
    let request: Request = serde_json::from_value(value).map_err(|error| {
        AppError::protocol(
            ErrorCode::InvalidRequest,
            format!("invalid request fields: {error}"),
        )
    })?;
    request.validate()?;
    Ok(request)
}

/// 从数据流之中解析出请求数据的字节
/// 如果出现了无效的utf-8编码则报错
pub fn parse_request_bytes(line: &[u8]) -> Result<Request> {
    let text = std::str::from_utf8(line)
        .map_err(|_| AppError::protocol(ErrorCode::InvalidUtf8, "request is not valid UTF-8"))?;
    parse_request_line(text)
}

/// 解析客户端收到的响应
pub fn parse_response_line(line: &str) -> Result<Response> {
    let payload = strip_line_terminator(line)?;
    ensure_frame_size(payload.len())?;
    if payload.is_empty() {
        return Err(AppError::protocol(
            ErrorCode::InvalidRequest,
            "response frame is empty",
        ));
    }
    let value: serde_json::Value = decode_json(payload)?;
    validate_response_fields(&value)?;
    serde_json::from_value(value).map_err(|error| {
        AppError::protocol(
            ErrorCode::InvalidRequest,
            format!("invalid response fields: {error}"),
        )
    })
}

/// 从数据流之中解析出响应数据的字节
/// 如果出现了无效的utf-8编码则报错
pub fn parse_response_bytes(line: &[u8]) -> Result<Response> {
    let text = std::str::from_utf8(line)
        .map_err(|_| AppError::protocol(ErrorCode::InvalidUtf8, "response is not valid UTF-8"))?;
    parse_response_line(text)
}

/// 将请求数据序列化, 同时添加上JSON Lines的分隔符
pub fn encode_request_line(request: &Request) -> Result<Vec<u8>> {
    request.validate()?;
    encode_json_line(request)
}

/// 将响应数据序列化, 同时添加上JSON Lines的分隔符
pub fn encode_response_line(response: &Response) -> Result<Vec<u8>> {
    encode_json_line(response)
}

/// 将value序列化为合规的JSON Line frame
pub fn encode_json_line<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut encoded = serde_json::to_vec(value)?;
    ensure_frame_size(encoded.len())?;
    encoded.push(b'\n');
    Ok(encoded)
}

/// 解析客户端输入的命令
pub fn parse_command(input: &str) -> Result<Request> {
    let tokens = tokenize(input.trim())?;
    let Some(command) = tokens.first().map(String::as_str) else {
        return Err(AppError::protocol(
            ErrorCode::MissingArgument,
            "missing command",
        ));
    };

    let request = match command {
        "set" if tokens.len() == 3 => Request::Set {
            key: tokens[1].clone(),
            value: tokens[2].clone(),
        },
        "get" if tokens.len() == 2 => Request::Get {
            key: tokens[1].clone(),
        },
        "delete" if tokens.len() == 2 => Request::Delete {
            key: tokens[1].clone(),
        },
        "keys" if tokens.len() == 1 => Request::Keys,
        "status" if tokens.len() == 1 => Request::Status,
        "ping" if tokens.len() == 1 => Request::Ping,
        "quit" if tokens.len() == 1 => Request::Quit,
        "set" | "get" | "delete" | "keys" | "status" | "ping" | "quit" => {
            let expected = match command {
                "set" => 3,
                "get" | "delete" => 2,
                _ => 1,
            };
            let code = if tokens.len() < expected {
                ErrorCode::MissingArgument
            } else {
                ErrorCode::ExtraArgument
            };
            return Err(AppError::protocol(code, "wrong number of arguments"));
        }
        _ => {
            return Err(AppError::protocol(
                ErrorCode::UnknownCommand,
                "unknown command",
            ));
        }
    };

    request.validate()?;
    Ok(request)
}

fn invalid_command(message: &str) -> AppError {
    AppError::protocol(ErrorCode::InvalidRequest, message)
}

fn ensure_frame_size(payload_bytes: usize) -> Result<()> {
    if payload_bytes > MAX_FRAME_BYTES {
        Err(AppError::protocol(
            ErrorCode::FrameTooLarge,
            format!("frame exceeds {MAX_FRAME_BYTES} bytes"),
        ))
    } else {
        Ok(())
    }
}

fn strip_line_terminator(line: &str) -> Result<&str> {
    if !line.ends_with('\n') {
        return Err(AppError::protocol(
            ErrorCode::InvalidRequest,
            "JSON Lines frame must end with LF",
        ));
    }
    let line = &line[..line.len() - 1];
    Ok(line.strip_suffix('\r').unwrap_or(line))
}

fn decode_json<T: DeserializeOwned>(payload: &str) -> Result<T> {
    serde_json::from_str(payload).map_err(|error| {
        AppError::protocol(ErrorCode::InvalidJson, format!("invalid JSON: {error}"))
    })
}

fn validate_request_fields(value: &serde_json::Value) -> Result<()> {
    let object = value.as_object().ok_or_else(|| {
        AppError::protocol(ErrorCode::InvalidRequest, "request must be a JSON object")
    })?;
    let command = match object.get("cmd") {
        None => {
            return Err(AppError::protocol(
                ErrorCode::MissingArgument,
                "missing cmd field",
            ));
        }
        Some(value) => value.as_str().ok_or_else(|| {
            AppError::protocol(ErrorCode::InvalidRequest, "request cmd must be a string")
        })?,
    };
    let allowed = match command {
        "set" => &["cmd", "key", "value"][..],
        "get" | "delete" => &["cmd", "key"][..],
        "keys" | "status" | "ping" | "quit" => &["cmd"][..],
        _ => {
            return Err(AppError::protocol(
                ErrorCode::UnknownCommand,
                format!("unknown command: {command}"),
            ));
        }
    };
    if let Some(field) = object
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        return Err(AppError::protocol(
            ErrorCode::ExtraArgument,
            format!("unknown request field: {field}"),
        ));
    }

    match command {
        "set" => {
            required_string(
                object,
                "key",
                ErrorCode::MissingArgument,
                ErrorCode::InvalidKey,
            )?;
            required_string(
                object,
                "value",
                ErrorCode::MissingArgument,
                ErrorCode::InvalidValue,
            )?;
        }
        "get" | "delete" => {
            required_string(
                object,
                "key",
                ErrorCode::MissingArgument,
                ErrorCode::InvalidKey,
            )?;
        }
        _ => {}
    }
    Ok(())
}

fn validate_response_fields(value: &serde_json::Value) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid_response("response must be a JSON object"))?;
    ensure_no_unknown_fields(object, &["ok", "data", "error"])?;

    let ok = object
        .get("ok")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| invalid_response("response ok must be a boolean"))?;
    let data = object.get("data");
    let error = object.get("error");
    match (ok, data, error) {
        (true, Some(data), None) => validate_response_data(data),
        (false, None, Some(error)) => validate_response_error(error),
        (true, _, _) => Err(invalid_response(
            "successful response must contain data and no error",
        )),
        (false, _, _) => Err(invalid_response(
            "error response must contain error and no data",
        )),
    }
}

fn validate_response_data(value: &serde_json::Value) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid_response("response data must be an object"))?;
    let kind = object
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid_response("response data kind must be a string"))?;

    match kind {
        "set" => {
            ensure_fields(object, &["kind", "replaced"])?;
            require_bool(object, "replaced")
        }
        "get" => {
            ensure_fields(object, &["kind", "value"])?;
            require_string(object, "value")
        }
        "delete" => {
            ensure_fields(object, &["kind", "deleted"])?;
            require_bool(object, "deleted")
        }
        "keys" => {
            ensure_fields(object, &["kind", "keys", "count"])?;
            let keys = object
                .get("keys")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| invalid_response("response keys must be an array"))?;
            if keys.iter().any(|key| !key.is_string()) {
                return Err(invalid_response("response keys must contain strings"));
            }
            let count = require_count(object, "count")?;
            if count != keys.len() {
                return Err(invalid_response("response keys count does not match keys"));
            }
            Ok(())
        }
        "status" => {
            ensure_fields(object, &["kind", "count"])?;
            require_count(object, "count").map(|_| ())
        }
        "ping" | "quit" => ensure_fields(object, &["kind"]),
        _ => Err(invalid_response("unknown response data kind")),
    }
}

fn validate_response_error(value: &serde_json::Value) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid_response("response error must be an object"))?;
    ensure_fields(object, &["code", "message"])?;
    let code = object
        .get("code")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid_response("response error code must be a string"))?;
    serde_json::from_str::<ErrorCode>(&format!("\"{code}\""))
        .map_err(|_| invalid_response("response error code is unknown"))?;
    require_string(object, "message")
}

fn ensure_fields(
    object: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
) -> Result<()> {
    ensure_no_unknown_fields(object, allowed)?;
    if allowed.iter().any(|field| !object.contains_key(*field)) {
        return Err(invalid_response("response is missing a required field"));
    }
    Ok(())
}

fn ensure_no_unknown_fields(
    object: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
) -> Result<()> {
    if object
        .keys()
        .any(|field| !allowed.contains(&field.as_str()))
    {
        return Err(invalid_response("response contains an unknown field"));
    }
    Ok(())
}

fn require_string(object: &serde_json::Map<String, serde_json::Value>, field: &str) -> Result<()> {
    if object
        .get(field)
        .and_then(serde_json::Value::as_str)
        .is_none()
    {
        return Err(invalid_response("response field must be a string"));
    }
    Ok(())
}

fn require_bool(object: &serde_json::Map<String, serde_json::Value>, field: &str) -> Result<()> {
    if object
        .get(field)
        .and_then(serde_json::Value::as_bool)
        .is_none()
    {
        return Err(invalid_response("response field must be a boolean"));
    }
    Ok(())
}

fn require_count(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<usize> {
    let count = object
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
        .ok_or_else(|| invalid_response("response count must be a non-negative integer"))?;
    Ok(count)
}

fn invalid_response(message: &str) -> AppError {
    AppError::protocol(ErrorCode::InvalidRequest, message)
}

fn required_string<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
    missing_code: ErrorCode,
    invalid_code: ErrorCode,
) -> Result<&'a str> {
    let value = object
        .get(field)
        .ok_or_else(|| AppError::protocol(missing_code, format!("missing {field} field")))?;
    value
        .as_str()
        .ok_or_else(|| AppError::protocol(invalid_code, format!("{field} must be a string")))
}

/// 将command进行分割处理
fn tokenize(input: &str) -> Result<Vec<String>> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index == bytes.len() {
            break;
        }

        if bytes[index] == b'"' {
            let start = index;
            index += 1;
            let mut escaped = false;
            let mut closed = false;
            while index < bytes.len() {
                let byte = bytes[index];
                index += 1;
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    closed = true;
                    break;
                }
            }
            if !closed || escaped {
                return Err(invalid_command("unterminated JSON quoted argument"));
            }
            let quoted = &input[start..index];
            let token = serde_json::from_str(quoted)
                .map_err(|_| invalid_command("quoted argument is not a valid JSON string"))?;
            tokens.push(token);
            if index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                return Err(invalid_command("quoted argument must end at whitespace"));
            }
        } else {
            let start = index;
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                if bytes[index] == b'"' {
                    return Err(invalid_command(
                        "quoted arguments must start at a token boundary",
                    ));
                }
                index += 1;
            }
            tokens.push(input[start..index].to_owned());
        }
    }

    Ok(tokens)
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
