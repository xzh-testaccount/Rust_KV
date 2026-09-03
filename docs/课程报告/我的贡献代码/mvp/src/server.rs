//! TCP server and connection dispatch layer.

use crate::error::{AppError, ErrorCode, Result};
use crate::persistence::PersistentStore;
use crate::protocol::{
    Frame, Request, Response, ResponseData, encode_response_line, parse_request_bytes, read_frame,
};
use std::io::{self, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const DEFAULT_DATA: &str = "data/kv.wal";

/// Server command-line configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    pub data: PathBuf,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 7878)),
            data: PathBuf::from(DEFAULT_DATA),
        }
    }
}

impl ServerConfig {
    /// Parses server arguments, returning `Ok(None)` for `--help`.
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
                    config.data = PathBuf::from(value);
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
    "Usage: kv-server [--bind HOST:PORT] [--data PATH]\n\nOptions:\n  --bind HOST:PORT  listening address (default 127.0.0.1:7878)\n  --data PATH       WAL path (default data/kv.wal)\n  -h, --help        show this help\n"
}

/// Opens the configured persistent store.
pub fn open(config: &ServerConfig) -> Result<PersistentStore> {
    PersistentStore::open(&config.data)
}

/// Binds and configures a non-blocking listener.
pub fn bind(config: &ServerConfig) -> Result<TcpListener> {
    let listener = TcpListener::bind(config.bind)?;
    listener.set_nonblocking(true)?;
    Ok(listener)
}

/// Runs the production server until its process is interrupted.
pub fn run(config: ServerConfig) -> Result<()> {
    let store = Arc::new(Mutex::new(open(&config)?));
    let listener = bind(&config)?;
    serve(listener, store, Arc::new(AtomicBool::new(false)))
}

/// Serves connections until `stop` is set, primarily for integration tests.
pub fn serve(
    listener: TcpListener,
    store: Arc<Mutex<PersistentStore>>,
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
                thread::spawn(move || handle_connection(stream, shared_store));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn handle_connection(stream: TcpStream, store: Arc<Mutex<PersistentStore>>) {
    let writer = match stream.try_clone() {
        Ok(writer) => writer,
        Err(_) => return,
    };
    let mut reader = BufReader::new(stream);
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
                Ok(request) => dispatch(request, &store),
                Err(error) => (Response::from_error(&error), false),
            },
        };

        if write_response(&mut writer, &response).is_err() {
            return;
        }
        if close {
            return;
        }
    }
}

fn dispatch(request: Request, store: &Arc<Mutex<PersistentStore>>) -> (Response, bool) {
    match request {
        Request::Ping => (Response::success(ResponseData::Ping), false),
        Request::Quit => (Response::success(ResponseData::Quit), true),
        Request::Set { key, value } => {
            let result = store
                .lock()
                .map_err(|_| AppError::Message("storage mutex poisoned".to_owned()))
                .and_then(|mut store| store.set(&key, &value));
            match result {
                Ok(outcome) => (
                    Response::success(ResponseData::Set {
                        replaced: outcome.replaced,
                    }),
                    false,
                ),
                Err(error) => (Response::from_error(&error), false),
            }
        }
        Request::Get { key } => {
            let result = store
                .lock()
                .map_err(|_| AppError::Message("storage mutex poisoned".to_owned()))
                .and_then(|store| store.get(&key));
            match result {
                Ok(value) => (Response::success(ResponseData::Get { value }), false),
                Err(error) => (Response::from_error(&error), false),
            }
        }
        Request::Delete { key } => {
            let result = store
                .lock()
                .map_err(|_| AppError::Message("storage mutex poisoned".to_owned()))
                .and_then(|mut store| store.delete(&key));
            match result {
                Ok(outcome) => (
                    Response::success(ResponseData::Delete {
                        deleted: outcome.deleted,
                    }),
                    false,
                ),
                Err(error) => (Response::from_error(&error), false),
            }
        }
        Request::Keys => {
            let result = store
                .lock()
                .map_err(|_| AppError::Message("storage mutex poisoned".to_owned()))
                .map(|store| (store.keys(), store.len()));
            match result {
                Ok((keys, count)) => (Response::success(ResponseData::Keys { keys, count }), false),
                Err(error) => (Response::from_error(&error), false),
            }
        }
        Request::Status => {
            let result = store
                .lock()
                .map_err(|_| AppError::Message("storage mutex poisoned".to_owned()))
                .map(|store| store.len());
            match result {
                Ok(count) => (Response::success(ResponseData::Status { count }), false),
                Err(error) => (Response::from_error(&error), false),
            }
        }
    }
}

fn write_response(writer: &mut TcpStream, response: &Response) -> io::Result<()> {
    let encoded = encode_response_line(response)
        .or_else(|_| {
            encode_response_line(&Response::error(
                ErrorCode::FrameTooLarge,
                "response frame is too large",
            ))
        })
        .map_err(|error| io::Error::other(error.to_string()))?;
    writer.write_all(&encoded)?;
    writer.flush()
}
