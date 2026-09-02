//! JSON Lines 协议、命令解析和帧读取。

use std::io::{self, BufRead};

use crate::error::{AppError, ErrorCode, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

use crate::error::{AppError, ErrorCode, Result};

/// 单帧最大负载，不包含结尾的 LF。
pub const MAX_FRAME_BYTES: usize = 65_536;
/// 键的最大 UTF-8 字节数。
pub const MAX_KEY_BYTES: usize = 256;
/// 值的最大 UTF-8 字节数。
pub const MAX_VALUE_BYTES: usize = 16 * 1024;

/// 从字节流中读取到的一帧。
#[derive(Debug, PartialEq, Eq)]
pub enum Frame {
    /// 完整的一行，包含结尾 LF。
    Line(Vec<u8>),
    /// 负载超过大小限制。
    TooLarge,
    /// 流正常结束且没有剩余数据。
    Eof,
    /// 流结束前没有读到 LF。
    Incomplete,
}

/// 同步读取一帧。
pub fn read_frame<R: BufRead>(reader: &mut R) -> io::Result<Frame> {
    let mut frame = Vec::with_capacity(MAX_FRAME_BYTES + 1);
    let mut oversized = false;

    loop {
        let (consumed, found_newline) = {
            let buffer = reader.fill_buf()?;
            if buffer.is_empty() {
                return Ok(frame_at_eof(frame, oversized));
            }

            let newline = buffer.iter().position(|byte| *byte == b'\n');
            let consumed = newline.map_or(buffer.len(), |index| index + 1);
            let payload_end = newline.unwrap_or(buffer.len());
            append_bounded(&mut frame, &mut oversized, &buffer[..payload_end]);
            (consumed, newline.is_some())
        };

        reader.consume(consumed);
        if found_newline {
            return Ok(finish_frame(frame, oversized));
        }
    }
}

/// 异步读取一帧。
pub async fn read_frame_async<R>(reader: &mut R) -> io::Result<Frame>
where
    R: AsyncBufRead + Unpin,
{
    let mut frame = Vec::with_capacity(MAX_FRAME_BYTES + 1);
    let mut oversized = false;

    loop {
        let (consumed, found_newline) = {
            let buffer = reader.fill_buf().await?;
            if buffer.is_empty() {
                return Ok(frame_at_eof(frame, oversized));
            }

            let newline = buffer.iter().position(|byte| *byte == b'\n');
            let consumed = newline.map_or(buffer.len(), |index| index + 1);
            let payload_end = newline.unwrap_or(buffer.len());
            append_bounded(&mut frame, &mut oversized, &buffer[..payload_end]);
            (consumed, newline.is_some())
        };

        reader.consume(consumed);
        if found_newline {
            return Ok(finish_frame(frame, oversized));
        }
    }
}

fn append_bounded(frame: &mut Vec<u8>, oversized: &mut bool, bytes: &[u8]) {
    let remaining = (MAX_FRAME_BYTES + 1).saturating_sub(frame.len());
    let copied = remaining.min(bytes.len());
    frame.extend_from_slice(&bytes[..copied]);
    *oversized |= copied < bytes.len();
}

fn finish_frame(mut frame: Vec<u8>, oversized: bool) -> Frame {
    if oversized {
        return Frame::TooLarge;
    }

    let payload_len = frame
        .len()
        .saturating_sub(usize::from(frame.last() == Some(&b'\r')));
    if payload_len > MAX_FRAME_BYTES {
        return Frame::TooLarge;
    }

    frame.push(b'\n');
    Frame::Line(frame)
}

fn frame_at_eof(frame: Vec<u8>, oversized: bool) -> Frame {
    let payload_len = frame
        .len()
        .saturating_sub(usize::from(frame.last() == Some(&b'\r')));
    if oversized || payload_len > MAX_FRAME_BYTES {
        Frame::TooLarge
    } else if frame.is_empty() {
        Frame::Eof
    } else {
        Frame::Incomplete
    }
}

/// 客户端可以发送的命令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "lowercase", deny_unknown_fields)]
pub enum Request {
    Set {
        key: String,
        value: String,
    },
    Get {
        key: String,
    },
    Delete {
        key: String,
    },
    Keys,
    Status,
    #[serde(rename = "storage_status")]
    StorageStatus,
    Compact,
    Ping,
    Quit,
}

impl Request {
    /// 反序列化后继续执行存储层的键值校验。
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Set { key, value } => {
                crate::storage::validate_key(key)?;
                crate::storage::validate_value(value)
            }
            Self::Get { key } | Self::Delete { key } => crate::storage::validate_key(key),
            Self::Keys
            | Self::Status
            | Self::StorageStatus
            | Self::Compact
            | Self::Ping
            | Self::Quit => Ok(()),
        }
    }
}

/// 成功响应携带的数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum ResponseData {
    Set {
        replaced: bool,
    },
    Get {
        value: String,
    },
    Delete {
        deleted: bool,
    },
    Keys {
        keys: Vec<String>,
        count: usize,
    },
    Status {
        count: usize,
    },
    #[serde(rename = "storage_status")]
    StorageStatus {
        entries: usize,
        wal_records: u64,
        wal_bytes: u64,
        snapshot_bytes: u64,
        last_sequence: u64,
        writable: bool,
    },
    Compact {
        entries: usize,
        compact_ms: u64,
        wal_records_before: u64,
        wal_bytes_before: u64,
        snapshot_bytes_before: u64,
        last_sequence_before: u64,
        wal_records_after: u64,
        wal_bytes_after: u64,
        snapshot_bytes_after: u64,
        last_sequence_after: u64,
    },
    Ping,
    Quit,
}

/// 失败响应携带的错误。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

/// 一个请求对应一个响应。
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

    fn validate(&self) -> Result<()> {
        match (self.ok, &self.data, &self.error) {
            (true, Some(ResponseData::Keys { keys, count }), None) if keys.len() != *count => {
                Err(invalid_response("keys count does not match keys"))
            }
            (true, Some(_), None) | (false, None, Some(_)) => Ok(()),
            (true, _, _) => Err(invalid_response(
                "successful response must contain data and no error",
            )),
            (false, _, _) => Err(invalid_response(
                "failed response must contain error and no data",
            )),
        }
    }
}

/// 解析一行请求。
pub fn parse_request_line(line: &str) -> Result<Request> {
    let payload = checked_payload(line, "request")?;
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

/// 从原始字节解析请求，并单独识别非法 UTF-8。
pub fn parse_request_bytes(line: &[u8]) -> Result<Request> {
    let text = std::str::from_utf8(line)
        .map_err(|_| AppError::protocol(ErrorCode::InvalidUtf8, "request is not valid UTF-8"))?;
    parse_request_line(text)
}

/// 解析一行响应。
pub fn parse_response_line(line: &str) -> Result<Response> {
    let payload = checked_payload(line, "response")?;
    let value: serde_json::Value = decode_json(payload)?;
    let response: Response = serde_json::from_value(value).map_err(|error| {
        AppError::protocol(
            ErrorCode::InvalidRequest,
            format!("invalid response fields: {error}"),
        )
    })?;
    response.validate()?;
    Ok(response)
}

/// 从原始字节解析响应。
pub fn parse_response_bytes(line: &[u8]) -> Result<Response> {
    let text = std::str::from_utf8(line)
        .map_err(|_| AppError::protocol(ErrorCode::InvalidUtf8, "response is not valid UTF-8"))?;
    parse_response_line(text)
}

/// 把请求编码成 JSON Lines。
pub fn encode_request_line(request: &Request) -> Result<Vec<u8>> {
    request.validate()?;
    encode_json_line(request)
}

/// 把响应编码成 JSON Lines。
pub fn encode_response_line(response: &Response) -> Result<Vec<u8>> {
    response.validate()?;
    encode_json_line(response)
}

/// 序列化 JSON，并补上 LF。
pub fn encode_json_line<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut encoded = serde_json::to_vec(value)?;
    ensure_frame_size(encoded.len())?;
    encoded.push(b'\n');
    Ok(encoded)
}

/// 把终端命令转换成协议请求。
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
        "storage-status" | "storage_status" if tokens.len() == 1 => Request::StorageStatus,
        "compact" if tokens.len() == 1 => Request::Compact,
        "ping" if tokens.len() == 1 => Request::Ping,
        "quit" if tokens.len() == 1 => Request::Quit,
        "set" | "get" | "delete" | "keys" | "status" | "storage-status" | "storage_status"
        | "compact" | "ping" | "quit" => {
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
                format!("unknown command: {command}"),
            ));
        }
    };

    request.validate()?;
    Ok(request)
}

fn checked_payload<'a>(line: &'a str, frame_name: &str) -> Result<&'a str> {
    if !line.ends_with('\n') {
        return Err(AppError::protocol(
            ErrorCode::InvalidRequest,
            format!("{frame_name} frame must end with LF"),
        ));
    }

    let without_lf = &line[..line.len() - 1];
    let payload = without_lf.strip_suffix('\r').unwrap_or(without_lf);
    ensure_frame_size(payload.len())?;
    if payload.is_empty() {
        return Err(AppError::protocol(
            ErrorCode::InvalidRequest,
            format!("{frame_name} frame is empty"),
        ));
    }
    Ok(payload)
}

fn decode_json(payload: &str) -> Result<serde_json::Value> {
    serde_json::from_str(payload).map_err(|error| {
        AppError::protocol(ErrorCode::InvalidJson, format!("invalid JSON: {error}"))
    })
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

fn validate_request_fields(value: &serde_json::Value) -> Result<()> {
    let object = value.as_object().ok_or_else(|| {
        AppError::protocol(ErrorCode::InvalidRequest, "request must be a JSON object")
    })?;
    let command = object
        .get("cmd")
        .ok_or_else(|| AppError::protocol(ErrorCode::MissingArgument, "missing cmd field"))?
        .as_str()
        .ok_or_else(|| {
            AppError::protocol(ErrorCode::InvalidRequest, "request cmd must be a string")
        })?;

    let allowed = match command {
        "set" => &["cmd", "key", "value"][..],
        "get" | "delete" => &["cmd", "key"][..],
        "keys" | "status" | "storage_status" | "compact" | "ping" | "quit" => &["cmd"][..],
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
            required_string(object, "key", ErrorCode::InvalidKey)?;
            required_string(object, "value", ErrorCode::InvalidValue)?;
        }
        "get" | "delete" => {
            required_string(object, "key", ErrorCode::InvalidKey)?;
        }
        _ => {}
    }
    Ok(())
}

fn required_string<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
    invalid_code: ErrorCode,
) -> Result<&'a str> {
    object
        .get(field)
        .ok_or_else(|| {
            AppError::protocol(ErrorCode::MissingArgument, format!("missing {field} field"))
        })?
        .as_str()
        .ok_or_else(|| AppError::protocol(invalid_code, format!("{field} must be a string")))
}

fn invalid_response(message: impl Into<String>) -> AppError {
    AppError::protocol(ErrorCode::InvalidRequest, message)
}

fn tokenize(input: &str) -> Result<Vec<String>> {
    let mut chars = input.chars().peekable();
    let mut tokens = Vec::new();

    while chars.peek().is_some() {
        while chars.peek().is_some_and(|ch| ch.is_whitespace()) {
            chars.next();
        }
        if chars.peek().is_none() {
            break;
        }

        let mut token = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            let mut closed = false;
            while let Some(ch) = chars.next() {
                match ch {
                    '"' => {
                        closed = true;
                        break;
                    }
                    '\\' => match chars.next() {
                        Some('"') => token.push('"'),
                        Some('\\') => token.push('\\'),
                        Some(other) => {
                            return Err(AppError::protocol(
                                ErrorCode::InvalidRequest,
                                format!("unsupported escape: \\{other}"),
                            ));
                        }
                        None => {
                            return Err(AppError::protocol(
                                ErrorCode::InvalidRequest,
                                "unfinished escape in quoted argument",
                            ));
                        }
                    },
                    other => token.push(other),
                }
            }
            if !closed {
                return Err(AppError::protocol(
                    ErrorCode::InvalidRequest,
                    "unterminated quoted argument",
                ));
            }
            if chars.peek().is_some_and(|ch| !ch.is_whitespace()) {
                return Err(AppError::protocol(
                    ErrorCode::InvalidRequest,
                    "quoted argument must be followed by whitespace",
                ));
            }
        } else {
            while chars.peek().is_some_and(|ch| !ch.is_whitespace()) {
                let ch = chars.next().expect("peek confirmed a character");
                if ch == '"' {
                    return Err(AppError::protocol(
                        ErrorCode::InvalidRequest,
                        "quote must start an argument",
                    ));
                }
                token.push(ch);
            }
        }
        tokens.push(token);
    }
    Ok(tokens)
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
    use std::io::{BufReader, Cursor};

    use super::*;

    #[test]
    fn frame_reader_handles_sticky_and_crlf_frames() {
        let input = b"{\"cmd\":\"ping\"}\r\n{\"cmd\":\"quit\"}\n";
        let mut reader = BufReader::new(Cursor::new(input));

        assert!(matches!(read_frame(&mut reader).unwrap(), Frame::Line(_)));
        assert!(matches!(read_frame(&mut reader).unwrap(), Frame::Line(_)));
        assert_eq!(read_frame(&mut reader).unwrap(), Frame::Eof);
    }

    #[test]
    fn oversized_frame_is_discarded_at_its_line_boundary() {
        let mut input = vec![b'x'; MAX_FRAME_BYTES + 1];
        input.extend_from_slice(b"\n{\"cmd\":\"ping\"}\n");
        let mut reader = BufReader::new(Cursor::new(input));

        assert_eq!(read_frame(&mut reader).unwrap(), Frame::TooLarge);
        assert!(matches!(read_frame(&mut reader).unwrap(), Frame::Line(_)));
    }

    #[test]
    fn oversized_unterminated_frame_is_reported_at_eof() {
        let input = vec![b'x'; MAX_FRAME_BYTES + 1];
        let mut reader = BufReader::new(Cursor::new(input));

        assert_eq!(read_frame(&mut reader).unwrap(), Frame::TooLarge);
    }

    #[test]
    fn request_and_response_keep_the_frozen_json_shape() {
        let request = Request::Set {
            key: "name".to_owned(),
            value: "Alice".to_owned(),
        };
        assert_eq!(
            String::from_utf8(encode_request_line(&request).unwrap()).unwrap(),
            "{\"cmd\":\"set\",\"key\":\"name\",\"value\":\"Alice\"}\n"
        );

        let response = Response::success(ResponseData::Set { replaced: false });
        assert_eq!(
            String::from_utf8(encode_response_line(&response).unwrap()).unwrap(),
            "{\"ok\":true,\"data\":{\"kind\":\"set\",\"replaced\":false}}\n"
        );

        assert_eq!(
            String::from_utf8(encode_request_line(&Request::StorageStatus).unwrap()).unwrap(),
            "{\"cmd\":\"storage_status\"}\n"
        );
        assert_eq!(
            parse_request_line("{\"cmd\":\"compact\"}\n").unwrap(),
            Request::Compact
        );
    }

    #[test]
    fn quoted_cli_value_preserves_spaces() {
        assert_eq!(
            parse_command("set course \"Rust systems programming\"").unwrap(),
            Request::Set {
                key: "course".to_owned(),
                value: "Rust systems programming".to_owned(),
            }
        );
        assert_eq!(
            parse_command("storage-status").unwrap(),
            Request::StorageStatus
        );
        assert_eq!(parse_command("compact").unwrap(), Request::Compact);
    }

    #[test]
    fn unknown_fields_and_commands_have_stable_codes() {
        let extra = parse_request_line("{\"cmd\":\"ping\",\"x\":1}\n").unwrap_err();
        assert_eq!(extra.code(), ErrorCode::ExtraArgument);

        let unknown = parse_request_line("{\"cmd\":\"drop\"}\n").unwrap_err();
        assert_eq!(unknown.code(), ErrorCode::UnknownCommand);
    }
}
