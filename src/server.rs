//! 同步线程与 Tokio 异步两套 TCP 服务端。

use std::{
    fmt, fs,
    future::Future,
    io::{self, BufReader as StdBufReader, BufWriter as StdBufWriter, Write},
    net::{SocketAddr, TcpListener as StdTcpListener, TcpStream as StdTcpStream},
    path::PathBuf,
    str::FromStr,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use tokio::{
    io::{AsyncWrite, AsyncWriteExt, BufReader as AsyncBufReader},
    net::{TcpListener as AsyncTcpListener, TcpStream as AsyncTcpStream},
    task::JoinSet,
};

use crate::{
    error::{AppError, ErrorCode, Result},
    persistence::PersistentStore,
    protocol::{
        Frame, Request, Response, ResponseData, encode_response_line, parse_request_bytes,
        read_frame, read_frame_async,
    },
};

pub const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:7878";
pub const DEFAULT_WAL_PATH: &str = "data/kv.wal";

/// 网络并发模型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeMode {
    Sync,
    Async,
}

impl fmt::Display for RuntimeMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Sync => "sync",
            Self::Async => "async",
        })
    }
}

impl FromStr for RuntimeMode {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "sync" => Ok(Self::Sync),
            "async" => Ok(Self::Async),
            _ => Err(cli_error(
                ErrorCode::InvalidRequest,
                format!("invalid runtime: {value}; expected sync or async"),
            )),
        }
    }
}

/// 共享存储的锁策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockStrategy {
    Mutex,
    RwLock,
}

impl fmt::Display for LockStrategy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Mutex => "mutex",
            Self::RwLock => "rwlock",
        })
    }
}

impl FromStr for LockStrategy {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "mutex" => Ok(Self::Mutex),
            "rwlock" => Ok(Self::RwLock),
            _ => Err(cli_error(
                ErrorCode::InvalidRequest,
                format!("invalid lock strategy: {value}; expected mutex or rwlock"),
            )),
        }
    }
}

/// 四种服务端组合共用同一份存储封装。
#[derive(Debug, Clone)]
pub enum SharedStore {
    Mutex(Arc<Mutex<PersistentStore>>),
    RwLock(Arc<RwLock<PersistentStore>>),
}

impl SharedStore {
    pub fn new(store: PersistentStore, strategy: LockStrategy) -> Self {
        match strategy {
            LockStrategy::Mutex => Self::Mutex(Arc::new(Mutex::new(store))),
            LockStrategy::RwLock => Self::RwLock(Arc::new(RwLock::new(store))),
        }
    }

    /// 只在一次存储操作期间持锁。
    fn execute(&self, request: Request) -> Result<ResponseData> {
        match self {
            Self::Mutex(store) => {
                let mut store = store
                    .lock()
                    .map_err(|_| AppError::storage("Mutex存储锁已中毒"))?;
                execute_exclusive(&mut store, request)
            }
            Self::RwLock(store) => match request {
                Request::Set { key, value } => {
                    let mut store = store
                        .write()
                        .map_err(|_| AppError::storage("RwLock写锁已中毒"))?;
                    let outcome = store.set(key, value)?;
                    Ok(ResponseData::Set {
                        replaced: outcome.replaced(),
                    })
                }
                Request::Delete { key } => {
                    let mut store = store
                        .write()
                        .map_err(|_| AppError::storage("RwLock写锁已中毒"))?;
                    store.delete(&key)?;
                    Ok(ResponseData::Delete { deleted: true })
                }
                Request::Get { key } => {
                    let store = store
                        .read()
                        .map_err(|_| AppError::storage("RwLock读锁已中毒"))?;
                    Ok(ResponseData::Get {
                        value: store.get(&key)?.to_owned(),
                    })
                }
                Request::Keys => {
                    let store = store
                        .read()
                        .map_err(|_| AppError::storage("RwLock读锁已中毒"))?;
                    let keys = store.keys();
                    let count = keys.len();
                    Ok(ResponseData::Keys { keys, count })
                }
                Request::Status => {
                    let store = store
                        .read()
                        .map_err(|_| AppError::storage("RwLock读锁已中毒"))?;
                    Ok(ResponseData::Status { count: store.len() })
                }
                Request::StorageStatus => {
                    let store = store
                        .read()
                        .map_err(|_| AppError::storage("RwLock读锁已中毒"))?;
                    storage_status(&store)
                }
                Request::Compact => {
                    let mut store = store
                        .write()
                        .map_err(|_| AppError::storage("RwLock写锁已中毒"))?;
                    compact_store(&mut store)
                }
                Request::Ping | Request::Quit => Err(AppError::protocol(
                    ErrorCode::InvalidRequest,
                    "command does not access storage",
                )),
            },
        }
    }
}

fn execute_exclusive(store: &mut PersistentStore, request: Request) -> Result<ResponseData> {
    match request {
        Request::Set { key, value } => {
            let outcome = store.set(key, value)?;
            Ok(ResponseData::Set {
                replaced: outcome.replaced(),
            })
        }
        Request::Get { key } => Ok(ResponseData::Get {
            value: store.get(&key)?.to_owned(),
        }),
        Request::Delete { key } => {
            store.delete(&key)?;
            Ok(ResponseData::Delete { deleted: true })
        }
        Request::Keys => {
            let keys = store.keys();
            let count = keys.len();
            Ok(ResponseData::Keys { keys, count })
        }
        Request::Status => Ok(ResponseData::Status { count: store.len() }),
        Request::StorageStatus => storage_status(store),
        Request::Compact => compact_store(store),
        Request::Ping | Request::Quit => Err(AppError::protocol(
            ErrorCode::InvalidRequest,
            "command does not access storage",
        )),
    }
}

fn storage_status(store: &PersistentStore) -> Result<ResponseData> {
    let stats = store.stats();
    Ok(ResponseData::StorageStatus {
        entries: stats.store.entries,
        wal_records: stats.wal_records,
        wal_bytes: stats.wal_bytes,
        snapshot_bytes: snapshot_file_bytes(store)?,
        last_sequence: store.last_sequence(),
        writable: stats.writable,
    })
}

fn compact_store(store: &mut PersistentStore) -> Result<ResponseData> {
    let snapshot_bytes_before = snapshot_file_bytes(store)?;
    let last_sequence_before = store.last_sequence();
    let started = Instant::now();
    let compacted = store.compact()?;
    let compact_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    Ok(ResponseData::Compact {
        entries: compacted.snapshot_entries,
        compact_ms,
        wal_records_before: compacted.records_before,
        wal_bytes_before: compacted.wal_bytes_before,
        snapshot_bytes_before,
        last_sequence_before,
        wal_records_after: compacted.records_after,
        wal_bytes_after: compacted.wal_bytes_after,
        snapshot_bytes_after: compacted.snapshot_bytes,
        last_sequence_after: compacted.last_seq,
    })
}

fn snapshot_file_bytes(store: &PersistentStore) -> Result<u64> {
    match fs::metadata(store.snapshot_path()) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(AppError::Io(error)),
    }
}

/// 服务端启动参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    pub wal_path: PathBuf,
    pub runtime: RuntimeMode,
    pub lock: LockStrategy,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND_ADDRESS
                .parse()
                .expect("built-in bind address must be valid"),
            wal_path: PathBuf::from(DEFAULT_WAL_PATH),
            runtime: RuntimeMode::Sync,
            lock: LockStrategy::Mutex,
        }
    }
}

impl ServerConfig {
    /// 解析进程参数；返回 `None` 表示只显示帮助。
    pub fn parse<I, S>(args: I) -> Result<Option<Self>>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut args = args.into_iter().map(Into::into);
        let _program = args.next();
        parse_server_args(args)
    }
}

pub fn parse_server_args<I, S>(args: I) -> Result<Option<ServerConfig>>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut config = ServerConfig::default();
    let mut bind_seen = false;
    let mut data_seen = false;
    let mut runtime_seen = false;
    let mut lock_seen = false;
    let mut help_seen = false;
    let mut args = args.into_iter().map(Into::into);

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--help" | "-h" => {
                if help_seen || bind_seen || data_seen || runtime_seen || lock_seen {
                    return Err(cli_error(
                        ErrorCode::ExtraArgument,
                        "--help must be used alone",
                    ));
                }
                help_seen = true;
            }
            "--bind" => {
                reject_option_after_help(help_seen)?;
                reject_duplicate(bind_seen, "--bind")?;
                let value = option_value(&mut args, "--bind", "an address")?;
                config.bind = value.parse().map_err(|_| {
                    cli_error(
                        ErrorCode::InvalidRequest,
                        format!("invalid bind address: {value}"),
                    )
                })?;
                bind_seen = true;
            }
            "--data" => {
                reject_option_after_help(help_seen)?;
                reject_duplicate(data_seen, "--data")?;
                let value = option_value(&mut args, "--data", "a WAL path")?;
                config.wal_path = PathBuf::from(value);
                data_seen = true;
            }
            "--runtime" => {
                reject_option_after_help(help_seen)?;
                reject_duplicate(runtime_seen, "--runtime")?;
                let value = option_value(&mut args, "--runtime", "sync or async")?;
                config.runtime = value.parse()?;
                runtime_seen = true;
            }
            "--lock" => {
                reject_option_after_help(help_seen)?;
                reject_duplicate(lock_seen, "--lock")?;
                let value = option_value(&mut args, "--lock", "mutex or rwlock")?;
                config.lock = value.parse()?;
                lock_seen = true;
            }
            option if option.starts_with('-') => {
                reject_option_after_help(help_seen)?;
                return Err(cli_error(
                    ErrorCode::UnknownCommand,
                    format!("unknown option: {option}"),
                ));
            }
            value => {
                return Err(cli_error(
                    ErrorCode::ExtraArgument,
                    format!("unexpected argument: {value}"),
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

pub fn help_text() -> &'static str {
    "Usage: kv-server [--bind HOST:PORT] [--data PATH] [--runtime sync|async] [--lock mutex|rwlock] [--help]\n\nOptions:\n  --bind HOST:PORT       listening address (default 127.0.0.1:7878)\n  --data PATH             WAL path (default data/kv.wal)\n  --runtime sync|async    network runtime (default sync)\n  --lock mutex|rwlock     shared-store lock (default mutex)\n  -h, --help              show this help\n"
}

/// 打开服务端唯一的持久化存储。
pub fn open(config: &ServerConfig) -> Result<PersistentStore> {
    PersistentStore::open(&config.wal_path)
}

/// 根据启动参数选择同步或异步网络实现。
pub fn run(config: ServerConfig) -> Result<()> {
    let store = SharedStore::new(open(&config)?, config.lock);
    match config.runtime {
        RuntimeMode::Sync => run_sync(config, store),
        RuntimeMode::Async => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            runtime.block_on(run_async(config, store))
        }
    }
}

fn run_sync(config: ServerConfig, store: SharedStore) -> Result<()> {
    let listener = StdTcpListener::bind(config.bind)?;
    announce_ready(listener.local_addr()?, &config)?;
    serve_sync(listener, store)
}

async fn run_async(config: ServerConfig, store: SharedStore) -> Result<()> {
    let listener = AsyncTcpListener::bind(config.bind).await?;
    announce_ready(listener.local_addr()?, &config)?;
    serve_async(listener, store).await
}

fn announce_ready(address: SocketAddr, config: &ServerConfig) -> Result<()> {
    println!(
        "kv-server listening on {address}, runtime={}, lock={}, WAL: {}",
        config.runtime,
        config.lock,
        config.wal_path.display()
    );
    io::stdout().flush()?;
    Ok(())
}

/// 同步 accept 循环，每个连接使用一个系统线程。
pub fn serve_sync(listener: StdTcpListener, store: SharedStore) -> Result<()> {
    for accepted in listener.incoming() {
        match accepted {
            Ok(stream) => spawn_sync_client(stream, store.clone()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(AppError::Io(error)),
        }
    }
    Ok(())
}

/// 可由测试控制退出的同步 accept 循环。
pub fn serve_sync_until(
    listener: StdTcpListener,
    store: SharedStore,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    listener.set_nonblocking(true)?;
    let mut clients = Vec::new();

    while !shutdown.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                let store = store.clone();
                clients.push(thread::spawn(move || {
                    if let Err(error) = handle_sync_connection(stream, store) {
                        eprintln!("client connection error: {error}");
                    }
                }));
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                reap_finished(&mut clients);
                thread::sleep(Duration::from_millis(2));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(AppError::Io(error)),
        }
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    while !clients.is_empty() && Instant::now() < deadline {
        reap_finished(&mut clients);
        if !clients.is_empty() {
            thread::sleep(Duration::from_millis(2));
        }
    }
    Ok(())
}

fn spawn_sync_client(stream: StdTcpStream, store: SharedStore) {
    thread::spawn(move || {
        if let Err(error) = handle_sync_connection(stream, store) {
            eprintln!("client connection error: {error}");
        }
    });
}

fn reap_finished(clients: &mut Vec<thread::JoinHandle<()>>) {
    let mut index = 0;
    while index < clients.len() {
        if clients[index].is_finished() {
            let handle = clients.swap_remove(index);
            let _ = handle.join();
        } else {
            index += 1;
        }
    }
}

fn handle_sync_connection(stream: StdTcpStream, store: SharedStore) -> Result<()> {
    // Windows 上非阻塞 listener 接受的套接字可能继承非阻塞状态。
    stream.set_nonblocking(false)?;
    let mut reader = StdBufReader::new(stream.try_clone()?);
    let mut writer = StdBufWriter::new(stream);

    loop {
        let (response, close) = match read_frame(&mut reader)? {
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
                Ok(request) => dispatch_sync(request, &store),
                Err(error) => (Response::from_error(&error), false),
            },
        };

        write_response_sync(&mut writer, &response)?;
        if close {
            return Ok(());
        }
    }
}

fn dispatch_sync(request: Request, store: &SharedStore) -> (Response, bool) {
    match request {
        Request::Ping => (Response::success(ResponseData::Ping), false),
        Request::Quit => (Response::success(ResponseData::Quit), true),
        request => match store.execute(request) {
            Ok(data) => (Response::success(data), false),
            Err(error) => (Response::from_error(&error), false),
        },
    }
}

fn write_response_sync<W>(writer: &mut W, response: &Response) -> io::Result<()>
where
    W: Write,
{
    let encoded = encoded_response(response)?;
    writer.write_all(&encoded)?;
    writer.flush()
}

/// Tokio accept 循环，每个连接使用一个异步任务。
pub async fn serve_async(listener: AsyncTcpListener, store: SharedStore) -> Result<()> {
    serve_async_until(listener, store, async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            eprintln!("failed to listen for Ctrl+C: {error}");
        }
    })
    .await
}

/// 可注入退出信号的 Tokio accept 循环。
pub async fn serve_async_until<F>(
    listener: AsyncTcpListener,
    store: SharedStore,
    shutdown: F,
) -> Result<()>
where
    F: Future<Output = ()>,
{
    tokio::pin!(shutdown);
    let mut clients = JoinSet::new();

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                clients.abort_all();
                while clients.join_next().await.is_some() {}
                return Ok(());
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        let store = store.clone();
                        clients.spawn(async move {
                            if let Err(error) = handle_async_connection(stream, store).await {
                                eprintln!("client connection error: {error}");
                            }
                        });
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(AppError::Io(error)),
                }
            }
            completed = clients.join_next(), if !clients.is_empty() => {
                if let Some(Err(error)) = completed
                    && !error.is_cancelled()
                {
                    eprintln!("client task failed: {error}");
                }
            }
        }
    }
}

/// 兼容原有调用名。
pub async fn serve_until<F>(
    listener: AsyncTcpListener,
    store: SharedStore,
    shutdown: F,
) -> Result<()>
where
    F: Future<Output = ()>,
{
    serve_async_until(listener, store, shutdown).await
}

async fn handle_async_connection(stream: AsyncTcpStream, store: SharedStore) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = AsyncBufReader::new(reader);

    loop {
        let (response, close) = match read_frame_async(&mut reader).await? {
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
                Ok(request) => dispatch_async(request, store.clone()).await,
                Err(error) => (Response::from_error(&error), false),
            },
        };

        write_response_async(&mut writer, &response).await?;
        if close {
            return Ok(());
        }
    }
}

async fn dispatch_async(request: Request, store: SharedStore) -> (Response, bool) {
    match request {
        Request::Ping => (Response::success(ResponseData::Ping), false),
        Request::Quit => (Response::success(ResponseData::Quit), true),
        request => match tokio::task::spawn_blocking(move || store.execute(request)).await {
            Ok(Ok(data)) => (Response::success(data), false),
            Ok(Err(error)) => (Response::from_error(&error), false),
            Err(error) => {
                let error = AppError::storage(format!("存储任务执行失败：{error}"));
                (Response::from_error(&error), false)
            }
        },
    }
}

async fn write_response_async<W>(writer: &mut W, response: &Response) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let encoded = encoded_response(response)?;
    writer.write_all(&encoded).await?;
    writer.flush().await
}

fn encoded_response(response: &Response) -> io::Result<Vec<u8>> {
    encode_response_line(response)
        .or_else(|_| {
            encode_response_line(&Response::error(
                ErrorCode::FrameTooLarge,
                "response frame is too large",
            ))
        })
        .map_err(|error| io::Error::other(error.to_string()))
}

fn option_value<I>(args: &mut I, option: &str, description: &str) -> Result<String>
where
    I: Iterator<Item = String>,
{
    let value = args.next().ok_or_else(|| {
        cli_error(
            ErrorCode::MissingArgument,
            format!("{option} requires {description}"),
        )
    })?;
    if value.is_empty() || value.starts_with('-') {
        return Err(cli_error(
            ErrorCode::MissingArgument,
            format!("{option} requires {description}"),
        ));
    }
    Ok(value)
}

fn reject_option_after_help(help_seen: bool) -> Result<()> {
    if help_seen {
        Err(cli_error(
            ErrorCode::ExtraArgument,
            "--help must be used alone",
        ))
    } else {
        Ok(())
    }
}

fn reject_duplicate(seen: bool, option: &str) -> Result<()> {
    if seen {
        Err(cli_error(
            ErrorCode::ExtraArgument,
            format!("duplicate {option} option"),
        ))
    } else {
        Ok(())
    }
}

fn cli_error(code: ErrorCode, message: impl Into<String>) -> AppError {
    AppError::protocol(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_architecture_contract() {
        let config = ServerConfig::default();
        assert_eq!(config.bind.to_string(), DEFAULT_BIND_ADDRESS);
        assert_eq!(config.wal_path, PathBuf::from(DEFAULT_WAL_PATH));
        assert_eq!(config.runtime, RuntimeMode::Sync);
        assert_eq!(config.lock, LockStrategy::Mutex);
    }

    #[test]
    fn all_server_options_are_checked() {
        let config = parse_server_args([
            "--bind",
            "127.0.0.1:9000",
            "--data",
            "x.wal",
            "--runtime",
            "async",
            "--lock",
            "rwlock",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(config.bind.to_string(), "127.0.0.1:9000");
        assert_eq!(config.wal_path, PathBuf::from("x.wal"));
        assert_eq!(config.runtime, RuntimeMode::Async);
        assert_eq!(config.lock, LockStrategy::RwLock);

        let duplicate = parse_server_args(["--runtime", "sync", "--runtime", "async"]).unwrap_err();
        assert_eq!(duplicate.code(), ErrorCode::ExtraArgument);

        let invalid = parse_server_args(["--lock", "spinlock"]).unwrap_err();
        assert_eq!(invalid.code(), ErrorCode::InvalidRequest);
    }
}
