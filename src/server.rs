//! TCP service-layer entry points.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::{AppError, ErrorCode, Result};
use crate::persistence::PersistentStore;
use crate::protocol::{
    Frame, Request, Response, ResponseData, encode_response_line, parse_request_bytes,
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
    "Usage: kv-server [--bind HOST:PORT] [--data PATH]\n\nOptions:\n  --bind HOST:PORT  listening address (default 127.0.0.1:7878)\n  --data PATH        WAL path (default data/kv.wal)\n  -h, --help        show this help\n"
}

/// 打开一个设定好的PersistentStore
pub fn open(config: &ServerConfig) -> Result<PersistentStore> {
    PersistentStore::open(&config.wal_path)
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
        .map_err(|e| std::io::Error::other(e.to_string()))?;
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
