//! TCP service-layer entry points.

use std::io;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crate::error::{AppError, ErrorCode, Result};
use crate::persistence::PersistentStore;
use crate::protocol::{
    Frame, Request, Response, ResponseData, encode_response_line, parse_request_bytes, read_frame,
    read_frame_async,
};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::signal;
use tokio::sync::Mutex;

pub const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:7878";
pub const DEFAULT_WAL_PATH: &str = "data/kv.wal";

/// Configuration passed to the future TCP listener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    pub wal_path: PathBuf,
    pub sync: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND_ADDRESS
                .parse()
                .expect("the built-in bind address must be valid"),
            wal_path: PathBuf::from(DEFAULT_WAL_PATH),
            sync: false, // 默认异步
        }
    }
}

impl ServerConfig {
    pub fn parse<I, S>(args: I) -> Result<Option<Self>>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut args = args.into_iter().map(Into::into);
        let _program = args.next();
        let mut config = Self::default();
        let mut bind_seen = false;
        let mut data_seen = false;
        let mut help_seen = false;
        let mut sync_seen = false;

        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--help" | "-h" => {
                    if help_seen || bind_seen || data_seen {
                        return Err(AppError::protocol(
                            ErrorCode::ExtraArgument,
                            "--help must be used alone",
                        ));
                    }
                    help_seen = true;
                }
                "--bind" => {
                    if help_seen {
                        return Err(AppError::protocol(
                            ErrorCode::ExtraArgument,
                            "--help must be used alone",
                        ));
                    }
                    if bind_seen {
                        return Err(AppError::protocol(
                            ErrorCode::ExtraArgument,
                            "duplicate --bind option",
                        ));
                    }
                    bind_seen = true;
                    let value = args.next().ok_or_else(|| {
                        AppError::protocol(ErrorCode::MissingArgument, "missing value for --bind")
                    })?;
                    config.bind = value.parse().map_err(|_| {
                        AppError::protocol(
                            ErrorCode::InvalidRequest,
                            format!("invalid bind address: {value}"),
                        )
                    })?;
                }
                "--data" => {
                    if help_seen {
                        return Err(AppError::protocol(
                            ErrorCode::ExtraArgument,
                            "--help must be used alone",
                        ));
                    }
                    if data_seen {
                        return Err(AppError::protocol(
                            ErrorCode::ExtraArgument,
                            "duplicate --data option",
                        ));
                    }
                    data_seen = true;
                    let value = args.next().ok_or_else(|| {
                        AppError::protocol(ErrorCode::MissingArgument, "missing value for --data")
                    })?;
                    config.wal_path = PathBuf::from(value);
                }
                "--sync" => {
                    if help_seen {
                        return Err(AppError::protocol(
                            ErrorCode::ExtraArgument,
                            "--help  must be used alone",
                        ));
                    }
                    if sync_seen {
                        return Err(AppError::protocol(
                            ErrorCode::ExtraArgument,
                            "duplicate --sync option",
                        ));
                    }
                    sync_seen = true;
                    config.sync = true;
                }
                _ if argument.starts_with('-') => {
                    if help_seen {
                        return Err(AppError::protocol(
                            ErrorCode::ExtraArgument,
                            "--help must be used alone",
                        ));
                    }
                    return Err(AppError::protocol(
                        ErrorCode::UnknownCommand,
                        format!("unknown option: {argument}"),
                    ));
                }
                _ => {
                    if help_seen {
                        return Err(AppError::protocol(
                            ErrorCode::ExtraArgument,
                            "--help must be used alone",
                        ));
                    }
                    return Err(AppError::protocol(
                        ErrorCode::InvalidRequest,
                        format!("unexpected argument: {argument}"),
                    ));
                }
            }
        }
        if help_seen {
            Ok(None)
        } else {
            Ok(Some(config))
        }
    }
}

pub fn help_text() -> &'static str {
    "Usage: kv-server [--bind HOST:PORT] [--data PATH] [--sync]\n\nOptions:\n  --bind HOST:PORT  listening address (default 127.0.0.1:7878)\n  --data PATH       WAL path (default data/kv.wal)\n  --sync            use synchronous (default asynchronous)\n  -h, --help        show this help\n"
}

/// 打开一个设定好的PersistentStore
pub fn open(config: &ServerConfig) -> Result<PersistentStore> {
    PersistentStore::open(&config.wal_path)
}

/// 同步运行服务器
pub fn _run(config: ServerConfig) -> Result<()> {
    let store = Arc::new(std::sync::Mutex::new(open(&config)?));
    let listener = std::net::TcpListener::bind(config.bind)?;
    listener.set_nonblocking(true)?;
    _server(listener, store, Arc::new(AtomicBool::new(false)))
}

/// 同步服务主循环
pub fn _server(
    listener: std::net::TcpListener,
    store: Arc<std::sync::Mutex<PersistentStore>>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    listener.set_nonblocking(true)?;
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                if stream.set_nonblocking(false).is_err() {
                    continue;
                }
                let shared_store = Arc::clone(&store);
                thread::spawn(move || _handle_connection(stream, shared_store));
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// 同步处理单个客户端连接
fn _handle_connection(stream: std::net::TcpStream, store: Arc<std::sync::Mutex<PersistentStore>>) {
    let writer = match stream.try_clone() {
        Ok(writer) => writer,
        Err(_) => return,
    };
    let mut reader = std::io::BufReader::new(stream);
    let mut writer = writer;

    loop {
        let frame = match read_frame(&mut reader) {
            Ok(frame) => frame,
            Err(_) => return,
        };
        let (response, close) = match frame {
            Frame::Eof => return,
            Frame::Incomplete => (
                Response::error(ErrorCode::InvalidRequest, "incomplete request frame"),
                true,
            ),
            Frame::TooLarge => (
                Response::error(ErrorCode::FrameTooLarge, "request frame is too large"),
                false,
            ),
            Frame::Line(line) => match parse_request_bytes(&line) {
                Ok(request) => _dispatch(request, &store),
                Err(e) => (Response::from_error(&e), false),
            },
        };

        if _write_response(&mut writer, &response).is_err() {
            return;
        }
        if close {
            return;
        }
    }
}

/// 同步分发请求
fn _dispatch(request: Request, store: &Arc<std::sync::Mutex<PersistentStore>>) -> (Response, bool) {
    match request {
        Request::Ping => (Response::success(ResponseData::Ping), false),
        Request::Quit => (Response::success(ResponseData::Quit), true),
        Request::Set { key, value } => {
            let result = store
                .lock()
                .map_err(|_| AppError::Storage {
                    message: "storage mutex poisoned".to_owned(),
                })
                .and_then(|mut store| store.set(key, value));
            match result {
                Ok(outcome) => (
                    Response::success(ResponseData::Set {
                        replaced: outcome.replaced(),
                    }),
                    false,
                ),
                Err(e) => (Response::from_error(&e), false),
            }
        }
        Request::Get { key } => {
            let result = store
                .lock()
                .map_err(|_| AppError::Storage {
                    message: "storage mutex poisoned".to_owned(),
                })
                .and_then(|store| store.get(&key).map(|v| v.to_string()));
            match result {
                Ok(value) => (
                    Response::success(ResponseData::Get {
                        value: value.to_string(),
                    }),
                    false,
                ),
                Err(e) => (Response::from_error(&e), false),
            }
        }
        Request::Delete { key } => {
            let result = store
                .lock()
                .map_err(|_| AppError::Storage {
                    message: "storage mutex poisoned".to_owned(),
                })
                .and_then(|mut store| store.delete(&key));
            match result {
                Ok(_) => (
                    Response::success(ResponseData::Delete { deleted: true }),
                    false,
                ),
                Err(e) => (Response::from_error(&e), false),
            }
        }
        Request::Keys => {
            let result = store
                .lock()
                .map_err(|_| AppError::Storage {
                    message: "storage mutex poisoned".to_owned(),
                })
                .map(|store| (store.keys(), store.len()));
            match result {
                Ok((keys, count)) => (Response::success(ResponseData::Keys { keys, count }), false),
                Err(e) => (Response::from_error(&e), false),
            }
        }
        Request::Status => {
            let result = store
                .lock()
                .map_err(|_| AppError::Storage {
                    message: "storage mutex poisoned".to_owned(),
                })
                .map(|store| store.len());
            match result {
                Ok(count) => (Response::success(ResponseData::Status { count }), false),
                Err(e) => (Response::from_error(&e), false),
            }
        }
    }
}

/// 同步写入响应
fn _write_response(writer: &mut std::net::TcpStream, response: &Response) -> io::Result<()> {
    let encoded = encode_response_line(response)
        .or_else(|_| {
            encode_response_line(&Response::error(
                ErrorCode::FrameTooLarge,
                "response frame is too large",
            ))
        })
        .map_err(|error| Error::other(error.to_string()))?;
    writer.write_all(&encoded)?;
    writer.flush()
}

/// 异步运行服务器
pub async fn run(config: ServerConfig) -> Result<()> {
    let store = Arc::new(Mutex::new(open(&config)?));
    let listener = TcpListener::bind(config.bind).await?;
    serve(listener, store).await?;
    Ok(())
}

/// 异步服务主循环
pub async fn serve(listener: TcpListener, store: Arc<Mutex<PersistentStore>>) -> Result<()> {
    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, _)) => {
                        let store_clone = Arc::clone(&store);
                        tokio::spawn(async move {
                            // 每一个客户端独立任务
                            if let Err(e) = handle_connection(stream, store_clone).await {
                                eprintln!("Client error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("Accept error: {}", e);
                        return Err(e.into());
                    }
                }
            }
            _ = signal::ctrl_c() => {
                println!("Shutdown signal received, exiting...");
                return Ok(());
            }
        }
    }
}

/// 异步处理单个客户端连接
async fn handle_connection(
    mut stream: TcpStream,
    store: Arc<Mutex<PersistentStore>>,
) -> Result<()> {
    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);

    loop {
        let frame = match read_frame_async(&mut reader).await {
            Ok(frame) => frame,
            Err(e) => {
                eprintln!("Read error: {}", e);
                return Ok(()); // 连接关闭或出错, 退出任务
            }
        };

        let (response, close) = match frame {
            Frame::Eof => return Ok(()),
            Frame::Incomplete => (
                Response::error(ErrorCode::InvalidRequest, "incomplete request frame"),
                true,
            ),
            Frame::TooLarge => (
                Response::error(ErrorCode::FrameTooLarge, "request frame is too large"),
                false,
            ),
            Frame::Line(line) => match parse_request_bytes(&line) {
                Ok(request) => dispatch(request, &store).await,
                Err(error) => (Response::from_error(&error), false),
            },
        };

        if write_response(&mut writer, &response).await.is_err() {
            return Ok(());
        }
        if close {
            return Ok(());
        }
    }
}

/// 异步分发请求
async fn dispatch(request: Request, store: &Arc<Mutex<PersistentStore>>) -> (Response, bool) {
    match request {
        Request::Ping => (Response::success(ResponseData::Ping), false),
        Request::Quit => (Response::success(ResponseData::Quit), true),
        Request::Set { key, value } => {
            let mut store = store.lock().await;
            match store.set(key, value) {
                Ok(outcome) => (
                    Response::success(ResponseData::Set {
                        replaced: outcome.replaced(),
                    }),
                    false,
                ),
                Err(error) => (Response::from_error(&error), false),
            }
        }
        Request::Get { key } => {
            let store = store.lock().await;
            match store.get(&key) {
                Ok(value) => (
                    Response::success(ResponseData::Get {
                        value: value.to_string(),
                    }),
                    false,
                ),
                Err(error) => (Response::from_error(&error), false),
            }
        }
        Request::Delete { key } => {
            let mut store = store.lock().await;
            match store.delete(&key) {
                Ok(_) => (
                    Response::success(ResponseData::Delete { deleted: true }),
                    false,
                ),
                Err(error) => {
                    // 如果错误是 NotFound, 表示键不存在, 删除不能发生
                    if error.code() == ErrorCode::NotFound {
                        (
                            Response::success(ResponseData::Delete { deleted: false }),
                            false,
                        )
                    } else {
                        (Response::from_error(&error), false)
                    }
                }
            }
        }
        Request::Keys => {
            let store = store.lock().await;
            let keys = store.keys();
            let count = store.len();
            (Response::success(ResponseData::Keys { keys, count }), false)
        }
        Request::Status => {
            let store = store.lock().await;
            let count = store.len();
            (Response::success(ResponseData::Status { count }), false)
        }
    }
}

/// 异步写入响应
async fn write_response(
    writer: &mut tokio::net::tcp::WriteHalf<'_>,
    response: &Response,
) -> std::io::Result<()> {
    let encoded = encode_response_line(response)
        .or_else(|_| {
            encode_response_line(&Response::error(
                ErrorCode::FrameTooLarge,
                "response frame is too large",
            ))
        })
        .map_err(|e| Error::other(e.to_string()))?;
    writer.write_all(&encoded).await?;
    writer.flush().await?;
    Ok(())
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
