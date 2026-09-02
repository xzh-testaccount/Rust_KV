//! 本地答辩实验控制器。
//!
//! 浏览器只访问本模块提供的 HTTP API。控制器再通过 TCP JSON Lines
//! 访问真实 KV 服务，并负责服务器进程、并发实验和崩溃恢复的生命周期。

use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    error::{AppError, Result},
    protocol::{
        Frame, Request, Response, ResponseData, encode_request_line, parse_response_bytes,
        read_frame,
    },
    server::{LockStrategy, RuntimeMode},
};

pub const DEFAULT_CONTROLLER_ADDRESS: &str = "127.0.0.1:7879";
const DEFAULT_SERVER_ADDRESS: &str = "127.0.0.1:7878";
const MAX_HTTP_HEADER_BYTES: usize = 32 * 1024;
const MAX_HTTP_BODY_BYTES: usize = 1024 * 1024;
const TCP_TIMEOUT: Duration = Duration::from_secs(3);

/// 控制器启动参数。
#[derive(Debug, Clone)]
pub struct ControllerConfig {
    pub bind: SocketAddr,
    pub server_bind: SocketAddr,
    pub wal_path: PathBuf,
    pub server_executable: PathBuf,
    pub artifact_root: PathBuf,
    pub auto_start: bool,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_CONTROLLER_ADDRESS
                .parse()
                .expect("默认控制器地址必须有效"),
            server_bind: DEFAULT_SERVER_ADDRESS
                .parse()
                .expect("默认服务器地址必须有效"),
            wal_path: PathBuf::from("data/kv.wal"),
            server_executable: sibling_binary("kv-server"),
            artifact_root: PathBuf::from("artifacts"),
            auto_start: true,
        }
    }
}

impl ControllerConfig {
    pub fn parse<I, S>(args: I) -> Result<Option<Self>>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut args = args.into_iter().map(Into::into);
        let _program = args.next();
        let mut config = Self::default();

        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--help" | "-h" => return Ok(None),
                "--bind" => {
                    let value = required_arg(&mut args, "--bind")?;
                    config.bind = value
                        .parse()
                        .map_err(|_| AppError::storage(format!("控制器监听地址无效：{value}")))?;
                }
                "--server-bind" => {
                    let value = required_arg(&mut args, "--server-bind")?;
                    config.server_bind = value
                        .parse()
                        .map_err(|_| AppError::storage(format!("KV 服务监听地址无效：{value}")))?;
                }
                "--data" => config.wal_path = PathBuf::from(required_arg(&mut args, "--data")?),
                "--server-bin" => {
                    config.server_executable =
                        PathBuf::from(required_arg(&mut args, "--server-bin")?)
                }
                "--artifacts" => {
                    config.artifact_root = PathBuf::from(required_arg(&mut args, "--artifacts")?)
                }
                "--no-auto-start" => config.auto_start = false,
                unknown => {
                    return Err(AppError::storage(format!("未知控制器参数：{unknown}")));
                }
            }
        }
        Ok(Some(config))
    }
}

pub fn help_text() -> &'static str {
    "用法：kv-controller [选项]\n\n\
     --bind HOST:PORT         HTTP 控制器地址，默认 127.0.0.1:7879\n\
     --server-bind HOST:PORT  主 KV 服务地址，默认 127.0.0.1:7878\n\
     --data PATH              主服务 WAL，默认 data/kv.wal\n\
     --server-bin PATH        kv-server 可执行文件路径\n\
     --artifacts PATH         实验结果目录，默认 artifacts\n\
     --no-auto-start          不自动启动主 KV 服务\n\
     -h, --help               显示帮助\n"
}

fn required_arg<I>(args: &mut I, option: &str) -> Result<String>
where
    I: Iterator<Item = String>,
{
    args.next()
        .filter(|value| !value.is_empty() && !value.starts_with('-'))
        .ok_or_else(|| AppError::storage(format!("{option} 缺少参数")))
}

fn sibling_binary(name: &str) -> PathBuf {
    let filename = format!("{name}{}", env::consts::EXE_SUFFIX);
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(&filename)))
        .unwrap_or_else(|| PathBuf::from(filename))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ManagedStatus {
    Online,
    Offline,
    Starting,
    Recovering,
    Error,
}

struct ManagedServer {
    child: Option<Child>,
    status: ManagedStatus,
    runtime: RuntimeMode,
    lock: LockStrategy,
    last_error: Option<String>,
    wal_replay_count: u64,
}

impl Default for ManagedServer {
    fn default() -> Self {
        Self {
            child: None,
            status: ManagedStatus::Offline,
            runtime: RuntimeMode::Sync,
            lock: LockStrategy::Mutex,
            last_error: None,
            wal_replay_count: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConcurrencyClientState {
    id: usize,
    state: String,
    completed: usize,
    total: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConcurrencyState {
    status: String,
    progress: f64,
    active_clients: usize,
    completed: usize,
    total: usize,
    successful: usize,
    failed: usize,
    elapsed_ms: u64,
    clients: Vec<ConcurrencyClientState>,
}

impl Default for ConcurrencyState {
    fn default() -> Self {
        Self {
            status: "IDLE".to_owned(),
            progress: 0.0,
            active_clients: 0,
            completed: 0,
            total: 0,
            successful: 0,
            failed: 0,
            elapsed_ms: 0,
            clients: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecoverySnapshot {
    count: usize,
    fingerprint: String,
    samples: Vec<KvEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct KvEntry {
    key: String,
    value: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryState {
    phase: String,
    progress: f64,
    before: Option<RecoverySnapshot>,
    after: Option<RecoverySnapshot>,
    lost: usize,
    verified: bool,
    wal_replay_count: u64,
    recovery_time_ms: u64,
    logs: Vec<String>,
}

impl Default for RecoveryState {
    fn default() -> Self {
        Self {
            phase: "IDLE".to_owned(),
            progress: 0.0,
            before: None,
            after: None,
            lost: 0,
            verified: false,
            wal_replay_count: 0,
            recovery_time_ms: 0,
            logs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RemoteBenchmarkPoint {
    clients: usize,
    qps: f64,
    p50: f64,
    p95: f64,
    p99: f64,
    success: usize,
    failed: usize,
    elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BenchmarkState {
    status: String,
    progress: f64,
    points: Vec<RemoteBenchmarkPoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    artifact_dir: Option<String>,
    reset_epoch: u64,
}

impl Default for BenchmarkState {
    fn default() -> Self {
        Self {
            status: "IDLE".to_owned(),
            progress: 0.0,
            points: Vec::new(),
            error: None,
            artifact_dir: None,
            reset_epoch: 0,
        }
    }
}

#[derive(Default)]
struct RecoveryVerification {
    before_entries: BTreeMap<String, String>,
}

struct ControllerInner {
    config: ControllerConfig,
    project_root: PathBuf,
    managed: Mutex<ManagedServer>,
    concurrency: Mutex<ConcurrencyState>,
    concurrency_stop: Arc<AtomicBool>,
    benchmark: Mutex<BenchmarkState>,
    benchmark_stop: Arc<AtomicBool>,
    recovery: Mutex<RecoveryState>,
    recovery_verification: Mutex<RecoveryVerification>,
}

impl ControllerInner {
    fn new(config: ControllerConfig) -> Self {
        Self {
            project_root: env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            config,
            managed: Mutex::new(ManagedServer::default()),
            concurrency: Mutex::new(ConcurrencyState::default()),
            concurrency_stop: Arc::new(AtomicBool::new(false)),
            benchmark: Mutex::new(BenchmarkState::default()),
            benchmark_stop: Arc::new(AtomicBool::new(false)),
            recovery: Mutex::new(RecoveryState::default()),
            recovery_verification: Mutex::new(RecoveryVerification::default()),
        }
    }
}

/// 启动只监听本机的 HTTP 控制器。
pub fn run(config: ControllerConfig) -> Result<()> {
    if !config.bind.ip().is_loopback() {
        return Err(AppError::storage("实验控制器只允许监听 127.0.0.1"));
    }
    let inner = Arc::new(ControllerInner::new(config));
    if inner.config.auto_start
        && let Err(error) = start_managed_server(&inner, RuntimeMode::Sync, LockStrategy::Mutex)
    {
        eprintln!("主 KV 服务未能自动启动：{}", error.message);
        eprintln!("控制器仍会启动，前端将真实显示 ERROR 状态。修复问题后可调用 restart。");
        if let Ok(mut managed) = inner.managed.lock() {
            managed.status = ManagedStatus::Error;
            managed.last_error = Some(error.message);
        }
    }

    let listener = TcpListener::bind(inner.config.bind)?;
    listener.set_nonblocking(true)?;
    let stop = Arc::new(AtomicBool::new(false));
    install_ctrl_c_handler(Arc::clone(&stop), inner.config.bind);
    println!("RustKV 实验控制器：http://{}", inner.config.bind);

    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let inner = Arc::clone(&inner);
                thread::spawn(move || {
                    if let Err(error) = handle_http_connection(stream, inner) {
                        eprintln!("controller HTTP error: {error}");
                    }
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(AppError::Io(error)),
        }
    }

    let _ = kill_managed_server(&inner);
    Ok(())
}

fn install_ctrl_c_handler(stop: Arc<AtomicBool>, address: SocketAddr) {
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        if let Ok(runtime) = runtime {
            let _ = runtime.block_on(tokio::signal::ctrl_c());
            stop.store(true, Ordering::Relaxed);
            let _ = TcpStream::connect_timeout(&address, Duration::from_millis(100));
        }
    });
}

#[derive(Debug)]
struct ApiError {
    status: u16,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: 400,
            code,
            message: message.into(),
        }
    }

    fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: 409,
            code,
            message: message.into(),
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: 503,
            code: "BACKEND_UNREACHABLE",
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: 500,
            code: "CONTROLLER_ERROR",
            message: message.into(),
        }
    }
}

struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

fn handle_http_connection(mut stream: TcpStream, inner: Arc<ControllerInner>) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let response = match read_http_request(&mut stream) {
        Ok(request) if request.method == "OPTIONS" => HttpResponse::empty(204),
        Ok(request) => match route_request(request, inner) {
            Ok(value) => HttpResponse::json(200, value),
            Err(error) => HttpResponse::json(
                error.status,
                json!({"error":{"code":error.code,"message":error.message}}),
            ),
        },
        Err(error) => HttpResponse::json(
            400,
            json!({"error":{"code":"INVALID_HTTP_REQUEST","message":error.to_string()}}),
        ),
    };
    response.write_to(&mut stream)
}

fn read_http_request(stream: &mut TcpStream) -> io::Result<HttpRequest> {
    let mut buffer = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        if let Some(index) = find_bytes(&buffer, b"\r\n\r\n") {
            break index + 4;
        }
        if buffer.len() > MAX_HTTP_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP 请求头过大",
            ));
        }
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "HTTP 请求头不完整",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
    };

    let header_text = std::str::from_utf8(&buffer[..header_end - 4])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "HTTP 请求头不是 UTF-8"))?;
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "缺少 HTTP 请求行"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let raw_path = parts.next().unwrap_or_default();
    let path = raw_path.split('?').next().unwrap_or(raw_path).to_owned();
    if method.is_empty() || path.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HTTP 请求行无效",
        ));
    }

    let mut content_length = 0_usize;
    for line in lines {
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = value
                .trim()
                .parse()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Content-Length 无效"))?;
        }
    }
    if content_length > MAX_HTTP_BODY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "HTTP 请求体过大",
        ));
    }
    while buffer.len() < header_end + content_length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "HTTP 请求体不完整",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
    }

    Ok(HttpRequest {
        method,
        path,
        body: buffer[header_end..header_end + content_length].to_vec(),
    })
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

impl HttpResponse {
    fn json(status: u16, value: Value) -> Self {
        Self {
            status,
            body: serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec()),
        }
    }

    fn empty(status: u16) -> Self {
        Self {
            status,
            body: Vec::new(),
        }
    }

    fn write_to(self, stream: &mut TcpStream) -> io::Result<()> {
        let reason = match self.status {
            200 => "OK",
            204 => "No Content",
            400 => "Bad Request",
            404 => "Not Found",
            409 => "Conflict",
            500 => "Internal Server Error",
            503 => "Service Unavailable",
            _ => "Error",
        };
        let headers = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: http://127.0.0.1:3000\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n",
            self.status,
            reason,
            self.body.len()
        );
        stream.write_all(headers.as_bytes())?;
        stream.write_all(&self.body)?;
        stream.flush()
    }
}

fn route_request(
    request: HttpRequest,
    inner: Arc<ControllerInner>,
) -> std::result::Result<Value, ApiError> {
    match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/api/kv") => proxy_kv(&request.body, inner.config.server_bind),
        ("GET", "/api/storage/state") => storage_state_json(inner.config.server_bind),
        ("POST", "/api/storage/compact") => compact_storage(inner.config.server_bind),
        ("GET", "/api/server/state") => server_state_json(&inner),
        ("POST", "/api/server/start") => {
            let selection = decode_optional_selection(&request.body)?;
            let runtime = parse_runtime(selection.runtime.as_deref().unwrap_or("sync"))?;
            let lock = parse_lock(selection.lock.as_deref().unwrap_or("mutex"))?;
            start_managed_server(&inner, runtime, lock)?;
            server_state_json(&inner)
        }
        ("POST", "/api/server/kill") => {
            kill_managed_server(&inner)?;
            server_state_json(&inner)
        }
        ("POST", "/api/server/restart") => {
            let (runtime, lock) = current_variant(&inner)?;
            let _ = kill_managed_server(&inner);
            start_managed_server(&inner, runtime, lock)?;
            server_state_json(&inner)
        }
        ("POST", "/api/experiment/start") | ("POST", "/api/experiment/concurrency/start") => {
            let input: ConcurrencyInput = decode_json(&request.body)?;
            start_concurrency(Arc::clone(&inner), input)?;
            Ok(json!({"accepted":true}))
        }
        ("POST", "/api/experiment/stop") | ("POST", "/api/experiment/concurrency/stop") => {
            inner.concurrency_stop.store(true, Ordering::Relaxed);
            Ok(json!({"stopped":true}))
        }
        ("GET", "/api/experiment/state") | ("GET", "/api/experiment/concurrency/state") => {
            let state = lock_or_api(&inner.concurrency)?.clone();
            serde_json::to_value(state).map_err(|error| ApiError::internal(error.to_string()))
        }
        ("GET", "/api/benchmark/capabilities") => Ok(benchmark_capabilities()),
        ("POST", "/api/benchmark/reset") => reset_benchmark(&inner),
        ("POST", "/api/benchmark/start") => {
            let input: BenchmarkInput = decode_json(&request.body)?;
            start_benchmark(Arc::clone(&inner), input)?;
            Ok(json!({"accepted":true}))
        }
        ("POST", "/api/benchmark/stop") => {
            inner.benchmark_stop.store(true, Ordering::Relaxed);
            Ok(json!({"stopped":true}))
        }
        ("GET", "/api/benchmark/state") => {
            let state = lock_or_api(&inner.benchmark)?.clone();
            serde_json::to_value(state).map_err(|error| ApiError::internal(error.to_string()))
        }
        ("POST", "/api/recovery/prepare") => {
            let input: RecoveryPrepareInput = decode_json(&request.body)?;
            prepare_recovery(&inner, input.count)?;
            recovery_state_json(&inner)
        }
        ("POST", "/api/recovery/kill") => {
            kill_for_recovery(&inner)?;
            recovery_state_json(&inner)
        }
        ("POST", "/api/recovery/restart") => {
            restart_for_recovery(Arc::clone(&inner))?;
            recovery_state_json(&inner)
        }
        ("GET", "/api/recovery/state") => recovery_state_json(&inner),
        _ => Err(ApiError {
            status: 404,
            code: "API_NOT_FOUND",
            message: format!("接口不存在：{} {}", request.method, request.path),
        }),
    }
}

fn decode_json<T: for<'de> Deserialize<'de>>(body: &[u8]) -> std::result::Result<T, ApiError> {
    serde_json::from_slice(body)
        .map_err(|error| ApiError::bad_request("INVALID_JSON", format!("JSON 请求体无效：{error}")))
}

#[derive(Default, Deserialize)]
struct ServerSelection {
    runtime: Option<String>,
    lock: Option<String>,
}

fn decode_optional_selection(body: &[u8]) -> std::result::Result<ServerSelection, ApiError> {
    if body.is_empty() {
        Ok(ServerSelection::default())
    } else {
        decode_json(body)
    }
}

fn parse_runtime(value: &str) -> std::result::Result<RuntimeMode, ApiError> {
    match value.to_ascii_lowercase().as_str() {
        "sync" => Ok(RuntimeMode::Sync),
        "async" => Ok(RuntimeMode::Async),
        _ => Err(ApiError::bad_request(
            "INVALID_EXPERIMENT_CONFIG",
            format!("runtime 只允许 sync/async：{value}"),
        )),
    }
}

fn parse_lock(value: &str) -> std::result::Result<LockStrategy, ApiError> {
    match value.to_ascii_lowercase().as_str() {
        "mutex" => Ok(LockStrategy::Mutex),
        "rwlock" => Ok(LockStrategy::RwLock),
        _ => Err(ApiError::bad_request(
            "INVALID_EXPERIMENT_CONFIG",
            format!("lock 只允许 mutex/rwlock：{value}"),
        )),
    }
}

fn runtime_name(runtime: RuntimeMode) -> &'static str {
    match runtime {
        RuntimeMode::Sync => "sync",
        RuntimeMode::Async => "async",
    }
}

fn lock_name(lock: LockStrategy) -> &'static str {
    match lock {
        LockStrategy::Mutex => "mutex",
        LockStrategy::RwLock => "rwlock",
    }
}

fn start_managed_server(
    inner: &Arc<ControllerInner>,
    runtime: RuntimeMode,
    lock: LockStrategy,
) -> std::result::Result<(), ApiError> {
    if !inner.config.server_executable.is_file() {
        return Err(ApiError::internal(format!(
            "找不到 kv-server：{}；请先执行 cargo build --bins",
            inner.config.server_executable.display()
        )));
    }
    if let Some(parent) = inner.config.wal_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|error| ApiError::internal(format!("无法创建 WAL 目录：{error}")))?;
    }

    {
        let mut managed = lock_or_api(&inner.managed)?;
        if let Some(child) = managed.child.as_mut() {
            match child.try_wait() {
                Ok(None) => return Ok(()),
                Ok(Some(_)) | Err(_) => managed.child = None,
            }
        }
        managed.status = ManagedStatus::Starting;
        managed.runtime = runtime;
        managed.lock = lock;
        managed.last_error = None;
        managed.wal_replay_count = count_wal_records(&inner.config.wal_path).unwrap_or(0);

        let child = Command::new(&inner.config.server_executable)
            .current_dir(&inner.project_root)
            .arg("--bind")
            .arg(inner.config.server_bind.to_string())
            .arg("--data")
            .arg(&inner.config.wal_path)
            .arg("--runtime")
            .arg(runtime_name(runtime))
            .arg("--lock")
            .arg(lock_name(lock))
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| ApiError::internal(format!("启动 kv-server 失败：{error}")))?;
        managed.child = Some(child);
    }

    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if ping_server(inner.config.server_bind).is_ok() {
            let mut managed = lock_or_api(&inner.managed)?;
            managed.status = ManagedStatus::Online;
            managed.last_error = None;
            return Ok(());
        }
        {
            let mut managed = lock_or_api(&inner.managed)?;
            if let Some(child) = managed.child.as_mut()
                && let Ok(Some(status)) = child.try_wait()
            {
                managed.child = None;
                managed.status = ManagedStatus::Error;
                managed.last_error = Some(format!("kv-server 提前退出：{status}"));
                return Err(ApiError::internal(
                    managed.last_error.clone().unwrap_or_default(),
                ));
            }
        }
        thread::sleep(Duration::from_millis(50));
    }

    let mut managed = lock_or_api(&inner.managed)?;
    if let Some(mut child) = managed.child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    managed.status = ManagedStatus::Error;
    managed.last_error = Some("kv-server 启动超时，可能是 WAL 损坏或端口被占用".to_owned());
    Err(ApiError::internal(
        managed.last_error.clone().unwrap_or_default(),
    ))
}

fn kill_managed_server(inner: &Arc<ControllerInner>) -> std::result::Result<(), ApiError> {
    let mut managed = lock_or_api(&inner.managed)?;
    if let Some(mut child) = managed.child.take() {
        child
            .kill()
            .map_err(|error| ApiError::internal(format!("强制终止服务失败：{error}")))?;
        child
            .wait()
            .map_err(|error| ApiError::internal(format!("等待服务退出失败：{error}")))?;
    }
    managed.status = ManagedStatus::Offline;
    Ok(())
}

fn current_variant(
    inner: &Arc<ControllerInner>,
) -> std::result::Result<(RuntimeMode, LockStrategy), ApiError> {
    let managed = lock_or_api(&inner.managed)?;
    Ok((managed.runtime, managed.lock))
}

fn server_state_json(inner: &Arc<ControllerInner>) -> std::result::Result<Value, ApiError> {
    let mut managed = lock_or_api(&inner.managed)?;
    if let Some(child) = managed.child.as_mut()
        && let Ok(Some(status)) = child.try_wait()
    {
        managed.child = None;
        managed.status = if status.success() {
            ManagedStatus::Offline
        } else {
            ManagedStatus::Error
        };
        managed.last_error = Some(format!("kv-server 已退出：{status}"));
    }
    if managed.status == ManagedStatus::Online && ping_server(inner.config.server_bind).is_err() {
        managed.status = ManagedStatus::Offline;
    }
    Ok(json!({
        "state": managed.status,
        "pid": managed.child.as_ref().map(Child::id),
        "runtime": runtime_name(managed.runtime),
        "lock": lock_name(managed.lock),
        "walReplayCount": managed.wal_replay_count,
        "error": managed.last_error,
    }))
}

fn count_wal_records(path: &Path) -> io::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let bytes = fs::read(path)?;
    Ok(bytes.iter().filter(|byte| **byte == b'\n').count() as u64)
}

fn proxy_kv(body: &[u8], address: SocketAddr) -> std::result::Result<Value, ApiError> {
    if body.is_empty() || body.len() > crate::protocol::MAX_FRAME_BYTES {
        return Err(ApiError::bad_request(
            "INVALID_REQUEST",
            "KV 请求体不能为空且不能超过 64 KiB",
        ));
    }
    let response = send_raw_tcp_request(address, body)?;
    serde_json::to_value(response).map_err(|error| ApiError::internal(error.to_string()))
}

fn storage_state_json(address: SocketAddr) -> std::result::Result<Value, ApiError> {
    let response = send_tcp_request(address, &Request::StorageStatus)?;
    let failure_message = storage_failure_message(&response);
    match (response.ok, response.data) {
        (
            true,
            Some(ResponseData::StorageStatus {
                entries,
                wal_records,
                wal_bytes,
                snapshot_bytes,
                last_sequence,
                writable,
            }),
        ) => Ok(json!({
            "engine": "snapshot-wal-v1",
            "entries": entries,
            "walRecords": wal_records,
            "walBytes": wal_bytes,
            "snapshotBytes": snapshot_bytes,
            "totalBytes": wal_bytes.saturating_add(snapshot_bytes),
            "lastSequence": last_sequence,
            "writable": writable,
        })),
        (false, _) => Err(ApiError::internal(failure_message)),
        _ => Err(ApiError::internal("STORAGE_STATUS 响应类型错误")),
    }
}

fn compact_storage(address: SocketAddr) -> std::result::Result<Value, ApiError> {
    let response = send_tcp_request(address, &Request::Compact)?;
    let failure_message = storage_failure_message(&response);
    match (response.ok, response.data) {
        (
            true,
            Some(ResponseData::Compact {
                entries,
                compact_ms,
                wal_records_before,
                wal_bytes_before,
                snapshot_bytes_before,
                last_sequence_before,
                wal_records_after,
                wal_bytes_after,
                snapshot_bytes_after,
                last_sequence_after,
            }),
        ) => Ok(json!({
            "compacted": true,
            "compactMs": compact_ms,
            "before": {
                "entries": entries,
                "walRecords": wal_records_before,
                "walBytes": wal_bytes_before,
                "snapshotBytes": snapshot_bytes_before,
                "totalBytes": wal_bytes_before.saturating_add(snapshot_bytes_before),
                "lastSequence": last_sequence_before,
            },
            "after": {
                "entries": entries,
                "walRecords": wal_records_after,
                "walBytes": wal_bytes_after,
                "snapshotBytes": snapshot_bytes_after,
                "totalBytes": wal_bytes_after.saturating_add(snapshot_bytes_after),
                "lastSequence": last_sequence_after,
            },
        })),
        (false, _) => Err(ApiError::internal(failure_message)),
        _ => Err(ApiError::internal("COMPACT 响应类型错误")),
    }
}

fn storage_failure_message(response: &Response) -> String {
    response.error.as_ref().map_or_else(
        || "存储操作失败，但服务端没有返回错误详情".to_owned(),
        |error| format!("{}: {}", error.code, error.message),
    )
}

fn send_raw_tcp_request(
    address: SocketAddr,
    payload: &[u8],
) -> std::result::Result<Response, ApiError> {
    let mut writer = TcpStream::connect_timeout(&address, TCP_TIMEOUT)
        .map_err(|error| ApiError::unavailable(format!("无法连接 KV 服务：{error}")))?;
    writer
        .set_read_timeout(Some(TCP_TIMEOUT))
        .map_err(|error| ApiError::unavailable(error.to_string()))?;
    writer
        .set_write_timeout(Some(TCP_TIMEOUT))
        .map_err(|error| ApiError::unavailable(error.to_string()))?;
    let mut reader = BufReader::new(
        writer
            .try_clone()
            .map_err(|error| ApiError::unavailable(error.to_string()))?,
    );
    writer
        .write_all(payload)
        .and_then(|_| writer.write_all(b"\n"))
        .and_then(|_| writer.flush())
        .map_err(|error| ApiError::unavailable(format!("发送 TCP 请求失败：{error}")))?;
    match read_frame(&mut reader)
        .map_err(|error| ApiError::unavailable(format!("读取 TCP 响应失败：{error}")))?
    {
        Frame::Line(bytes) => parse_response_bytes(&bytes)
            .map_err(|error| ApiError::unavailable(error.client_message())),
        Frame::TooLarge => Err(ApiError::unavailable("KV 服务响应超过 64 KiB")),
        Frame::Eof | Frame::Incomplete => Err(ApiError::unavailable("KV 服务提前断开连接")),
    }
}

fn ping_server(address: SocketAddr) -> std::result::Result<(), ApiError> {
    let response = send_tcp_request(address, &Request::Ping)?;
    match (response.ok, response.data) {
        (true, Some(ResponseData::Ping)) => Ok(()),
        _ => Err(ApiError::unavailable("KV 服务 Ping 响应无效")),
    }
}

fn send_tcp_request(
    address: SocketAddr,
    request: &Request,
) -> std::result::Result<Response, ApiError> {
    let mut session = TcpSession::connect(address)?;
    session.request(request)
}

struct TcpSession {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
}

impl TcpSession {
    fn connect(address: SocketAddr) -> std::result::Result<Self, ApiError> {
        let writer = TcpStream::connect_timeout(&address, TCP_TIMEOUT)
            .map_err(|error| ApiError::unavailable(format!("无法连接 KV 服务：{error}")))?;
        writer
            .set_read_timeout(Some(TCP_TIMEOUT))
            .map_err(|error| ApiError::unavailable(error.to_string()))?;
        writer
            .set_write_timeout(Some(TCP_TIMEOUT))
            .map_err(|error| ApiError::unavailable(error.to_string()))?;
        writer
            .set_nodelay(true)
            .map_err(|error| ApiError::unavailable(error.to_string()))?;
        let reader = BufReader::new(
            writer
                .try_clone()
                .map_err(|error| ApiError::unavailable(error.to_string()))?,
        );
        Ok(Self { writer, reader })
    }

    fn request(&mut self, request: &Request) -> std::result::Result<Response, ApiError> {
        let line = encode_request_line(request)
            .map_err(|error| ApiError::bad_request("INVALID_REQUEST", error.client_message()))?;
        self.writer
            .write_all(&line)
            .and_then(|_| self.writer.flush())
            .map_err(|error| ApiError::unavailable(format!("发送 TCP 请求失败：{error}")))?;
        match read_frame(&mut self.reader)
            .map_err(|error| ApiError::unavailable(format!("读取 TCP 响应失败：{error}")))?
        {
            Frame::Line(bytes) => parse_response_bytes(&bytes)
                .map_err(|error| ApiError::unavailable(error.client_message())),
            Frame::TooLarge => Err(ApiError::unavailable("KV 服务响应超过 64 KiB")),
            Frame::Eof | Frame::Incomplete => Err(ApiError::unavailable("KV 服务提前断开连接")),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConcurrencyInput {
    clients: usize,
    requests_per_client: usize,
    workload: String,
}

fn start_concurrency(
    inner: Arc<ControllerInner>,
    input: ConcurrencyInput,
) -> std::result::Result<(), ApiError> {
    if !(1..=100).contains(&input.clients) {
        return Err(ApiError::bad_request(
            "INVALID_EXPERIMENT_CONFIG",
            "clients 必须在 1..=100",
        ));
    }
    if !(1..=10_000).contains(&input.requests_per_client) {
        return Err(ApiError::bad_request(
            "INVALID_EXPERIMENT_CONFIG",
            "requestsPerClient 必须在 1..=10000",
        ));
    }
    let workload = WorkloadProfile::parse(&input.workload)?;
    ping_server(inner.config.server_bind)?;
    {
        let mut state = lock_or_api(&inner.concurrency)?;
        if state.status == "RUNNING" {
            return Err(ApiError::conflict(
                "EXPERIMENT_RUNNING",
                "已有并发实验正在运行",
            ));
        }
        *state = ConcurrencyState {
            status: "RUNNING".to_owned(),
            progress: 0.0,
            active_clients: input.clients,
            completed: 0,
            total: input.clients * input.requests_per_client,
            successful: 0,
            failed: 0,
            elapsed_ms: 0,
            clients: (1..=input.clients)
                .map(|id| ConcurrencyClientState {
                    id,
                    state: "IDLE".to_owned(),
                    completed: 0,
                    total: input.requests_per_client,
                })
                .collect(),
        };
    }
    inner.concurrency_stop.store(false, Ordering::Relaxed);

    thread::spawn(move || run_concurrency(inner, input, workload));
    Ok(())
}

fn run_concurrency(
    inner: Arc<ControllerInner>,
    input: ConcurrencyInput,
    workload: WorkloadProfile,
) {
    let started = Instant::now();
    let barrier = Arc::new(Barrier::new(input.clients));
    let mut handles = Vec::with_capacity(input.clients);
    for client_index in 0..input.clients {
        let inner = Arc::clone(&inner);
        let barrier = Arc::clone(&barrier);
        let requests = input.requests_per_client;
        handles.push(thread::spawn(move || {
            let key = format!("concurrency:{}:{client_index}", unix_millis());
            let mut session = TcpSession::connect(inner.config.server_bind).ok();
            if let Some(connection) = session.as_mut() {
                let _ = connection.request(&Request::Set {
                    key: key.clone(),
                    value: "seed".to_owned(),
                });
            }
            barrier.wait();

            for request_index in 0..requests {
                if inner.concurrency_stop.load(Ordering::Relaxed) {
                    break;
                }
                let write = workload.is_write(request_index, client_index);
                let request = if write {
                    Request::Set {
                        key: key.clone(),
                        value: format!("value-{request_index:05}"),
                    }
                } else {
                    Request::Get { key: key.clone() }
                };
                let success = session
                    .as_mut()
                    .and_then(|connection| connection.request(&request).ok())
                    .is_some_and(|response| response.ok);
                if let Ok(mut state) = inner.concurrency.lock() {
                    state.completed += 1;
                    if success {
                        state.successful += 1;
                    } else {
                        state.failed += 1;
                    }
                    state.progress = state.completed as f64 / state.total.max(1) as f64 * 100.0;
                    state.elapsed_ms = duration_millis(started.elapsed());
                    if let Some(client) = state.clients.get_mut(client_index) {
                        client.completed += 1;
                        client.state = if client.completed == client.total {
                            "DONE".to_owned()
                        } else if write {
                            "WRITE".to_owned()
                        } else {
                            "READ".to_owned()
                        };
                    }
                }
            }
        }));
    }
    for handle in handles {
        let _ = handle.join();
    }
    if let Ok(mut state) = inner.concurrency.lock() {
        state.active_clients = 0;
        state.elapsed_ms = duration_millis(started.elapsed());
        if inner.concurrency_stop.load(Ordering::Relaxed) {
            state.status = "STOPPED".to_owned();
        } else if state.completed == state.total {
            state.status = "COMPLETED".to_owned();
            state.progress = 100.0;
        } else {
            state.status = "INTERRUPTED".to_owned();
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum WorkloadProfile {
    Read,
    Mixed,
    Write,
}

impl WorkloadProfile {
    fn parse(value: &str) -> std::result::Result<Self, ApiError> {
        match value.to_ascii_lowercase().as_str() {
            "read" | "read-heavy" => Ok(Self::Read),
            "mixed" => Ok(Self::Mixed),
            "write" | "write-heavy" => Ok(Self::Write),
            _ => Err(ApiError::bad_request(
                "INVALID_EXPERIMENT_CONFIG",
                format!("workload 无效：{value}"),
            )),
        }
    }

    fn is_write(self, request_index: usize, client_index: usize) -> bool {
        let sample = deterministic_sample(request_index as u64, client_index as u64) % 100;
        sample
            >= match self {
                Self::Read => 90,
                Self::Mixed => 50,
                Self::Write => 10,
            }
    }
}

fn deterministic_sample(index: u64, salt: u64) -> u64 {
    let mut value = index
        .wrapping_add(salt.wrapping_mul(0x9E37_79B9))
        .wrapping_add(0xA076_1D64_78BD_642F);
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value.wrapping_mul(0x94D0_49BB_1331_11EB) ^ (value >> 31)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryPrepareInput {
    count: usize,
}

fn prepare_recovery(
    inner: &Arc<ControllerInner>,
    count: usize,
) -> std::result::Result<(), ApiError> {
    if !(1..=2_000).contains(&count) {
        return Err(ApiError::bad_request(
            "INVALID_EXPERIMENT_CONFIG",
            "恢复实验键数必须在 1..=2000",
        ));
    }
    ping_server(inner.config.server_bind)?;
    {
        let state = lock_or_api(&inner.recovery)?;
        if state.phase == "RECOVERING" {
            return Err(ApiError::conflict("RECOVERY_RUNNING", "恢复实验正在运行"));
        }
    }

    let existing = fetch_all_entries(inner.config.server_bind)?;
    let mut session = TcpSession::connect(inner.config.server_bind)?;
    for key in existing
        .keys()
        .filter(|key| key.starts_with("recovery_demo:"))
    {
        let response = session.request(&Request::Delete { key: key.clone() })?;
        if !response.ok {
            return Err(ApiError::internal(format!("清理旧恢复键失败：{key}")));
        }
    }
    for index in 0..count {
        let key = format!("recovery_demo:{index:04}");
        let value = format!("durable-value-{:04}", (index * 17 + 11) % 9973);
        let response = session.request(&Request::Set { key, value })?;
        if !response.ok {
            return Err(ApiError::internal("准备恢复数据时 SET 失败"));
        }
    }

    let entries = fetch_all_entries(inner.config.server_bind)?;
    let snapshot = snapshot_from_entries(&entries, None);
    let wal_records = count_wal_records(&inner.config.wal_path)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    lock_or_api(&inner.recovery_verification)?.before_entries = entries;
    *lock_or_api(&inner.recovery)? = RecoveryState {
        phase: "PREPARED".to_owned(),
        progress: 25.0,
        before: Some(snapshot.clone()),
        after: None,
        lost: 0,
        verified: false,
        wal_replay_count: wal_records,
        recovery_time_ms: 0,
        logs: vec![
            format!("[SEED] 真实写入 {count} 个恢复实验键"),
            "[DURABILITY] 每条成功响应均已写入 WAL 并执行 flush + sync_data".to_owned(),
            format!(
                "[VERIFY] 控制器记录 Before 指纹 {}，只用于恢复结果校验",
                snapshot.fingerprint
            ),
        ],
    };
    Ok(())
}

fn kill_for_recovery(inner: &Arc<ControllerInner>) -> std::result::Result<(), ApiError> {
    {
        let state = lock_or_api(&inner.recovery)?;
        if state.phase != "PREPARED" {
            return Err(ApiError::conflict(
                "RECOVERY_NOT_PREPARED",
                "请先准备恢复实验数据",
            ));
        }
    }
    kill_managed_server(inner)?;
    let mut state = lock_or_api(&inner.recovery)?;
    state.phase = "CRASHED".to_owned();
    state.progress = 50.0;
    state
        .logs
        .push("[KILL] 已强制终止真实 kv-server 子进程".to_owned());
    state
        .logs
        .push("[MEMORY] 进程内 BTreeMap 已消失".to_owned());
    state
        .logs
        .push("[DISK] Snapshot 与 WAL 文件保留，等待重新加载".to_owned());
    Ok(())
}

fn restart_for_recovery(inner: Arc<ControllerInner>) -> std::result::Result<(), ApiError> {
    {
        let mut state = lock_or_api(&inner.recovery)?;
        if state.phase != "CRASHED" {
            return Err(ApiError::conflict(
                "RECOVERY_NOT_CRASHED",
                "只有强制终止后才能执行持久化恢复",
            ));
        }
        state.phase = "RECOVERING".to_owned();
        state.progress = 60.0;
        state
            .logs
            .push("[BOOT] 正在启动新的 kv-server 进程".to_owned());
        state
            .logs
            .push("[SOURCE] 恢复来源为持久化 Snapshot + WAL".to_owned());
    }
    {
        let mut managed = lock_or_api(&inner.managed)?;
        managed.status = ManagedStatus::Recovering;
    }
    thread::spawn(move || complete_recovery(inner));
    Ok(())
}

fn complete_recovery(inner: Arc<ControllerInner>) {
    let started = Instant::now();
    let result = (|| -> std::result::Result<(), ApiError> {
        let (runtime, lock) = current_variant(&inner)?;
        start_managed_server(&inner, runtime, lock)?;
        {
            let mut state = lock_or_api(&inner.recovery)?;
            state.progress = 85.0;
            state
                .logs
                .push("[REPLAY] PersistentStore::open 已加载 Snapshot 并重放增量 WAL".to_owned());
        }
        let after_entries = fetch_all_entries(inner.config.server_bind)?;
        let before_entries = lock_or_api(&inner.recovery_verification)?
            .before_entries
            .clone();
        let before_snapshot = snapshot_from_entries(&before_entries, None);
        let after_snapshot = snapshot_from_entries(&after_entries, Some(&before_snapshot.samples));
        let lost = before_entries
            .iter()
            .filter(|(key, value)| after_entries.get(*key) != Some(*value))
            .count();
        let verified = lost == 0
            && before_entries.len() == after_entries.len()
            && before_snapshot.fingerprint == after_snapshot.fingerprint;
        let wal_records = count_wal_records(&inner.config.wal_path)
            .map_err(|error| ApiError::internal(error.to_string()))?;
        let mut state = lock_or_api(&inner.recovery)?;
        state.phase = "VERIFIED".to_owned();
        state.progress = 100.0;
        state.before = Some(before_snapshot.clone());
        state.after = Some(after_snapshot.clone());
        state.lost = lost;
        state.verified = verified;
        state.wal_replay_count = wal_records;
        state.recovery_time_ms = duration_millis(started.elapsed());
        state.logs.push(format!(
            "[VERIFY] Before {} Keys / After {} Keys",
            before_snapshot.count, after_snapshot.count
        ));
        state.logs.push(format!(
            "[HASH] {} → {}",
            before_snapshot.fingerprint, after_snapshot.fingerprint
        ));
        state.logs.push(if verified {
            "[PASS] 数量、键值与指纹全部一致".to_owned()
        } else {
            format!("[FAIL] 检测到 {lost} 个键丢失或值不一致")
        });
        Ok(())
    })();
    if let Err(error) = result
        && let Ok(mut state) = inner.recovery.lock()
    {
        state.phase = "ERROR".to_owned();
        state.logs.push(format!("[ERROR] {}", error.message));
    }
}

fn recovery_state_json(inner: &Arc<ControllerInner>) -> std::result::Result<Value, ApiError> {
    let state = lock_or_api(&inner.recovery)?.clone();
    serde_json::to_value(state).map_err(|error| ApiError::internal(error.to_string()))
}

fn fetch_all_entries(
    address: SocketAddr,
) -> std::result::Result<BTreeMap<String, String>, ApiError> {
    let mut session = TcpSession::connect(address)?;
    let keys_response = session.request(&Request::Keys)?;
    let keys = match (keys_response.ok, keys_response.data) {
        (true, Some(ResponseData::Keys { keys, .. })) => keys,
        (false, _) => return Err(ApiError::internal("KEYS 请求失败")),
        _ => return Err(ApiError::internal("KEYS 响应类型错误")),
    };
    let mut entries = BTreeMap::new();
    for key in keys {
        let response = session.request(&Request::Get { key: key.clone() })?;
        match (response.ok, response.data) {
            (true, Some(ResponseData::Get { value })) => {
                entries.insert(key, value);
            }
            _ => return Err(ApiError::internal("GET 响应类型错误")),
        }
    }
    Ok(entries)
}

fn snapshot_from_entries(
    entries: &BTreeMap<String, String>,
    preferred_samples: Option<&[KvEntry]>,
) -> RecoverySnapshot {
    let samples = preferred_samples
        .map(|items| {
            items
                .iter()
                .map(|item| KvEntry {
                    key: item.key.clone(),
                    value: entries.get(&item.key).cloned().unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_else(|| {
            entries
                .iter()
                .filter(|(key, _)| key.starts_with("recovery_demo:"))
                .take(3)
                .map(|(key, value)| KvEntry {
                    key: key.clone(),
                    value: value.clone(),
                })
                .collect()
        });
    RecoverySnapshot {
        count: entries.len(),
        fingerprint: fingerprint(entries),
        samples,
    }
}

fn fingerprint(entries: &BTreeMap<String, String>) -> String {
    let mut hash = 0x811C_9DC5_u32;
    for (key, value) in entries {
        for byte in key
            .bytes()
            .chain(std::iter::once(b'='))
            .chain(value.bytes())
            .chain(std::iter::once(b';'))
        {
            hash ^= u32::from(byte);
            hash = hash.wrapping_mul(16_777_619);
        }
    }
    format!("0x{hash:08X}")
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BenchmarkInput {
    scales: Vec<usize>,
    requests_per_scale: usize,
    runtime: String,
    lock: String,
    workload: String,
}

fn benchmark_capabilities() -> Value {
    json!({
        "runtimes":[
            {"id":"sync","label":"Sync / Thread-per-connection","available":true},
            {"id":"async","label":"Async / Tokio","available":true}
        ],
        "lockStrategies":[
            {"id":"mutex","label":"Mutex","available":true},
            {"id":"rwlock","label":"RwLock","available":true}
        ],
        "workloads":[
            {"id":"read-heavy","readPct":90,"writePct":10},
            {"id":"mixed","readPct":50,"writePct":50},
            {"id":"write-heavy","readPct":10,"writePct":90}
        ],
        "fixedConditions":{
            "datasetKeys":10000,
            "valueBytes":128,
            "requestsPerScale":10000,
            "persistence":"WAL + flush + sync_data",
            "protocol":"JSON Lines",
            "network":"localhost",
            "seed":20260902,
            "warmupRuns":1,
            "measuredRuns":5
        }
    })
}

fn reset_benchmark(inner: &Arc<ControllerInner>) -> std::result::Result<Value, ApiError> {
    let mut state = lock_or_api(&inner.benchmark)?;
    if state.status == "RUNNING" {
        return Err(ApiError::conflict(
            "BENCHMARK_RUNNING",
            "性能实验运行中，不能重置环境",
        ));
    }
    let next_epoch = state.reset_epoch.saturating_add(1);
    *state = BenchmarkState {
        reset_epoch: next_epoch,
        ..BenchmarkState::default()
    };
    inner.benchmark_stop.store(false, Ordering::Relaxed);
    Ok(json!({"reset":true,"resetEpoch":next_epoch}))
}

fn start_benchmark(
    inner: Arc<ControllerInner>,
    input: BenchmarkInput,
) -> std::result::Result<(), ApiError> {
    if input.scales.is_empty()
        || input
            .scales
            .iter()
            .any(|clients| !(1..=100).contains(clients))
        || !(1..=100_000).contains(&input.requests_per_scale)
    {
        return Err(ApiError::bad_request(
            "INVALID_EXPERIMENT_CONFIG",
            "scales 必须在 1..=100，requestsPerScale 必须在 1..=100000",
        ));
    }
    let runtime = parse_runtime(&input.runtime)?;
    let lock = parse_lock(&input.lock)?;
    let workload = WorkloadProfile::parse(&input.workload)?;
    {
        let mut state = lock_or_api(&inner.benchmark)?;
        if state.status == "RUNNING" {
            return Err(ApiError::conflict(
                "BENCHMARK_RUNNING",
                "已有性能实验正在运行",
            ));
        }
        state.status = "RUNNING".to_owned();
        state.progress = 0.0;
        state.points.clear();
        state.error = None;
        state.artifact_dir = None;
    }
    inner.benchmark_stop.store(false, Ordering::Relaxed);
    thread::spawn(move || run_benchmark_job(inner, input, runtime, lock, workload));
    Ok(())
}

// benchmark.rs 完成真实运行与证据包；此处只负责异步状态编排。
fn run_benchmark_job(
    inner: Arc<ControllerInner>,
    input: BenchmarkInput,
    runtime: RuntimeMode,
    lock: LockStrategy,
    workload: WorkloadProfile,
) {
    let total = input.scales.len().max(1);
    for (index, clients) in input.scales.iter().copied().enumerate() {
        if inner.benchmark_stop.load(Ordering::Relaxed) {
            if let Ok(mut state) = inner.benchmark.lock() {
                state.status = "STOPPED".to_owned();
            }
            return;
        }
        let result = crate::benchmark::run_benchmark(
            crate::benchmark::BenchmarkConfig {
                server_executable: inner.config.server_executable.clone(),
                artifact_root: inner.config.artifact_root.clone(),
                runtime,
                lock,
                workload: match workload {
                    WorkloadProfile::Read => crate::benchmark::BenchmarkWorkload::ReadHeavy,
                    WorkloadProfile::Mixed => crate::benchmark::BenchmarkWorkload::Mixed,
                    WorkloadProfile::Write => crate::benchmark::BenchmarkWorkload::WriteHeavy,
                },
                clients,
                requests: input.requests_per_scale,
                dataset_keys: 10_000,
                value_bytes: 128,
                seed: 20_260_902,
                warmup_runs: 1,
                measured_runs: 5,
            },
            Arc::clone(&inner.benchmark_stop),
            {
                let inner = Arc::clone(&inner);
                move |event| {
                    let fraction = match event.phase {
                        crate::benchmark::BenchmarkPhase::Preparing => 0.0,
                        crate::benchmark::BenchmarkPhase::Warmup => {
                            if event.total == 0 {
                                0.1
                            } else {
                                event.run as f64 / event.total as f64 * 0.1
                            }
                        }
                        crate::benchmark::BenchmarkPhase::Measured => {
                            0.1 + event.run as f64 / event.total.max(1) as f64 * 0.9
                        }
                        crate::benchmark::BenchmarkPhase::Completed => 1.0,
                    };
                    if let Ok(mut state) = inner.benchmark.lock() {
                        state.progress = ((index as f64 + fraction) / total as f64) * 100.0;
                    }
                }
            },
        );
        match result {
            Ok(outcome) => {
                if let Ok(mut state) = inner.benchmark.lock() {
                    state.points.push(RemoteBenchmarkPoint {
                        clients,
                        qps: outcome.throughput_qps,
                        p50: outcome.p50_ms,
                        p95: outcome.p95_ms,
                        p99: outcome.p99_ms,
                        success: usize::try_from(outcome.success).unwrap_or(usize::MAX),
                        failed: usize::try_from(outcome.failed).unwrap_or(usize::MAX),
                        elapsed_ms: outcome.elapsed_ms.round().clamp(0.0, u64::MAX as f64) as u64,
                    });
                    state.artifact_dir = Some(outcome.artifact_dir.display().to_string());
                    state.progress = (index + 1) as f64 / total as f64 * 100.0;
                }
            }
            Err(error) => {
                if let Ok(mut state) = inner.benchmark.lock() {
                    state.status = if inner.benchmark_stop.load(Ordering::Relaxed) {
                        "STOPPED".to_owned()
                    } else {
                        "INTERRUPTED".to_owned()
                    };
                    state.error = Some(error.to_string());
                }
                return;
            }
        }
    }
    if let Ok(mut state) = inner.benchmark.lock() {
        state.status = "COMPLETED".to_owned();
        state.progress = 100.0;
    }
}

fn lock_or_api<T>(mutex: &Mutex<T>) -> std::result::Result<std::sync::MutexGuard<'_, T>, ApiError> {
    mutex
        .lock()
        .map_err(|_| ApiError::internal("控制器共享状态锁已损坏"))
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn duration_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_order_independent_because_entries_are_sorted() {
        let mut first = BTreeMap::new();
        first.insert("b".to_owned(), "2".to_owned());
        first.insert("a".to_owned(), "1".to_owned());
        let mut second = BTreeMap::new();
        second.insert("a".to_owned(), "1".to_owned());
        second.insert("b".to_owned(), "2".to_owned());
        assert_eq!(fingerprint(&first), fingerprint(&second));
    }

    #[test]
    fn workload_ratios_are_deterministic() {
        let first: Vec<_> = (0..1000)
            .map(|index| WorkloadProfile::Read.is_write(index, 7))
            .collect();
        let second: Vec<_> = (0..1000)
            .map(|index| WorkloadProfile::Read.is_write(index, 7))
            .collect();
        assert_eq!(first, second);
        let writes = first.iter().filter(|write| **write).count();
        assert!((50..=150).contains(&writes));
    }
}
