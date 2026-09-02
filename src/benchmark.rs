//! 真实 TCP 基准实验与证据包生成。

use std::{
    fmt,
    fs::{self, File},
    io::{self, BufReader, BufWriter, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    protocol::{
        Frame, MAX_VALUE_BYTES, Request, encode_request_line, parse_response_bytes, read_frame,
    },
    server::{LockStrategy, RuntimeMode},
};

static ARTIFACT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// 基准实验的读写比例。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkWorkload {
    ReadHeavy,
    Mixed,
    WriteHeavy,
}

impl BenchmarkWorkload {
    pub fn read_percent(self) -> u8 {
        match self {
            Self::ReadHeavy => 90,
            Self::Mixed => 50,
            Self::WriteHeavy => 10,
        }
    }

    pub fn write_percent(self) -> u8 {
        100 - self.read_percent()
    }
}

/// 一组可重复执行的真实服务器基准配置。
#[derive(Debug, Clone)]
pub struct BenchmarkConfig {
    pub server_executable: PathBuf,
    pub artifact_root: PathBuf,
    pub runtime: RuntimeMode,
    pub lock: LockStrategy,
    pub workload: BenchmarkWorkload,
    pub clients: usize,
    pub requests: usize,
    pub dataset_keys: usize,
    pub value_bytes: usize,
    pub seed: u64,
    pub warmup_runs: usize,
    pub measured_runs: usize,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            server_executable: default_server_executable(),
            artifact_root: PathBuf::from("artifacts"),
            runtime: RuntimeMode::Async,
            lock: LockStrategy::Mutex,
            workload: BenchmarkWorkload::Mixed,
            clients: 1,
            requests: 10_000,
            dataset_keys: 10_000,
            value_bytes: 128,
            seed: 0x5255_5354_4B56_0001,
            warmup_runs: 1,
            measured_runs: 5,
        }
    }
}

/// 一轮正式实验的原始汇总。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkPoint {
    pub run: usize,
    pub clients: usize,
    pub requested: u64,
    pub attempted: u64,
    pub completed: u64,
    pub success: u64,
    pub failed: u64,
    pub elapsed_ms: f64,
    pub throughput_qps: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
}

/// 全部正式轮次的真实结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkOutcome {
    pub artifact_dir: PathBuf,
    pub runtime: String,
    pub lock: String,
    pub workload: BenchmarkWorkload,
    pub clients: usize,
    pub requests_per_run: usize,
    pub measured_runs: Vec<BenchmarkPoint>,
    pub requested: u64,
    pub attempted: u64,
    pub completed: u64,
    pub success: u64,
    pub failed: u64,
    pub elapsed_ms: f64,
    pub throughput_qps: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
}

/// 用于控制层展示真实进度。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkProgress {
    pub phase: BenchmarkPhase,
    pub run: usize,
    pub total: usize,
    pub point: Option<BenchmarkPoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkPhase {
    Preparing,
    Warmup,
    Measured,
    Completed,
}

/// 基准失败会保留已生成证据，不会补造结果。
#[derive(Debug)]
pub enum BenchmarkError {
    Cancelled {
        artifact_dir: PathBuf,
    },
    Failed {
        message: String,
        artifact_dir: Option<PathBuf>,
    },
}

impl BenchmarkError {
    fn failed(message: impl Into<String>) -> Self {
        Self::Failed {
            message: message.into(),
            artifact_dir: None,
        }
    }

    fn with_artifact(self, artifact_dir: &Path) -> Self {
        match self {
            Self::Cancelled { .. } => Self::Cancelled {
                artifact_dir: artifact_dir.to_path_buf(),
            },
            Self::Failed { message, .. } => Self::Failed {
                message,
                artifact_dir: Some(artifact_dir.to_path_buf()),
            },
        }
    }

    pub fn artifact_dir(&self) -> Option<&Path> {
        match self {
            Self::Cancelled { artifact_dir } => Some(artifact_dir),
            Self::Failed { artifact_dir, .. } => artifact_dir.as_deref(),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }
}

impl fmt::Display for BenchmarkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled { artifact_dir } => {
                write!(
                    formatter,
                    "benchmark cancelled; artifacts: {}",
                    artifact_dir.display()
                )
            }
            Self::Failed {
                message,
                artifact_dir: Some(path),
            } => write!(formatter, "{message}; artifacts: {}", path.display()),
            Self::Failed {
                message,
                artifact_dir: None,
            } => formatter.write_str(message),
        }
    }
}

impl std::error::Error for BenchmarkError {}

impl From<io::Error> for BenchmarkError {
    fn from(error: io::Error) -> Self {
        Self::failed(error.to_string())
    }
}

impl From<serde_json::Error> for BenchmarkError {
    fn from(error: serde_json::Error) -> Self {
        Self::failed(error.to_string())
    }
}

/// 启动真实服务器并完成预热、正式实验和证据落盘。
pub fn run_benchmark<F>(
    config: BenchmarkConfig,
    cancelled: Arc<AtomicBool>,
    mut progress: F,
) -> Result<BenchmarkOutcome, BenchmarkError>
where
    F: FnMut(BenchmarkProgress),
{
    validate_config(&config)?;
    fs::create_dir_all(&config.artifact_root)?;
    let artifact_dir = create_unique_directory(&config.artifact_root, "benchmark")?;
    let raw_dir = artifact_dir.join("raw");
    fs::create_dir_all(&raw_dir)?;

    progress(BenchmarkProgress {
        phase: BenchmarkPhase::Preparing,
        run: 0,
        total: config.warmup_runs + config.measured_runs,
        point: None,
    });

    let result = run_benchmark_inner(&config, &artifact_dir, &raw_dir, &cancelled, &mut progress);
    match result {
        Ok(outcome) => Ok(outcome),
        Err(error) => {
            let error = error.with_artifact(&artifact_dir);
            let failure = FailureArtifact {
                cancelled: error.is_cancelled(),
                message: error.to_string(),
                captured_at_unix_ms: unix_time_millis(),
            };
            let _ = write_json(&artifact_dir.join("failure.json"), &failure);
            Err(error)
        }
    }
}

fn run_benchmark_inner<F>(
    config: &BenchmarkConfig,
    artifact_dir: &Path,
    raw_dir: &Path,
    cancelled: &Arc<AtomicBool>,
    progress: &mut F,
) -> Result<BenchmarkOutcome, BenchmarkError>
where
    F: FnMut(BenchmarkProgress),
{
    write_json(&artifact_dir.join("environment.json"), &environment())?;
    write_json(
        &artifact_dir.join("config.json"),
        &ConfigArtifact::from_config(config),
    )?;

    let workspace = TemporaryWorkspace::new()?;
    let baseline = workspace.path.join("baseline.wal");
    write_deterministic_baseline(
        &baseline,
        config.dataset_keys,
        config.value_bytes,
        config.seed,
    )?;

    for run in 1..=config.warmup_runs {
        check_cancelled(cancelled, artifact_dir)?;
        progress(BenchmarkProgress {
            phase: BenchmarkPhase::Warmup,
            run,
            total: config.warmup_runs,
            point: None,
        });
        let round_dir = workspace.path.join(format!("warmup-{run:02}"));
        let round = run_round(config, &baseline, &round_dir, cancelled)?;
        if round.cancelled {
            return Err(BenchmarkError::Cancelled {
                artifact_dir: artifact_dir.to_path_buf(),
            });
        }
        progress(BenchmarkProgress {
            phase: BenchmarkPhase::Warmup,
            run,
            total: config.warmup_runs,
            point: Some(round.point(run)),
        });
    }

    let mut points = Vec::with_capacity(config.measured_runs);
    let mut all_latencies_ns = Vec::new();
    for run in 1..=config.measured_runs {
        check_cancelled(cancelled, artifact_dir)?;
        progress(BenchmarkProgress {
            phase: BenchmarkPhase::Measured,
            run,
            total: config.measured_runs,
            point: None,
        });
        let round_dir = workspace.path.join(format!("measured-{run:02}"));
        let round = run_round(config, &baseline, &round_dir, cancelled)?;
        let point = round.point(run);
        if round.cancelled {
            write_json(&raw_dir.join(format!("partial-run-{run:02}.json")), &point)?;
            return Err(BenchmarkError::Cancelled {
                artifact_dir: artifact_dir.to_path_buf(),
            });
        }
        write_json(&raw_dir.join(format!("run-{run:02}.json")), &point)?;
        all_latencies_ns.extend_from_slice(&round.latencies_ns);
        points.push(point.clone());
        progress(BenchmarkProgress {
            phase: BenchmarkPhase::Measured,
            run,
            total: config.measured_runs,
            point: Some(point),
        });
    }

    let outcome = summarize(config, artifact_dir, points, all_latencies_ns)?;
    write_json(&artifact_dir.join("summary.json"), &outcome)?;
    progress(BenchmarkProgress {
        phase: BenchmarkPhase::Completed,
        run: config.measured_runs,
        total: config.measured_runs,
        point: None,
    });
    Ok(outcome)
}

fn validate_config(config: &BenchmarkConfig) -> Result<(), BenchmarkError> {
    if !config.server_executable.is_file() {
        return Err(BenchmarkError::failed(format!(
            "kv-server executable does not exist: {}",
            config.server_executable.display()
        )));
    }
    if config.clients == 0 {
        return Err(BenchmarkError::failed("clients must be greater than zero"));
    }
    if config.requests == 0 {
        return Err(BenchmarkError::failed("requests must be greater than zero"));
    }
    if config.clients > config.requests {
        return Err(BenchmarkError::failed(
            "clients cannot exceed requests in one benchmark run",
        ));
    }
    if config.dataset_keys == 0 {
        return Err(BenchmarkError::failed(
            "dataset_keys must be greater than zero",
        ));
    }
    if config.value_bytes == 0 || config.value_bytes > MAX_VALUE_BYTES {
        return Err(BenchmarkError::failed(format!(
            "value_bytes must be between 1 and {MAX_VALUE_BYTES}"
        )));
    }
    if config.measured_runs == 0 {
        return Err(BenchmarkError::failed(
            "measured_runs must be greater than zero",
        ));
    }
    Ok(())
}

fn run_round(
    config: &BenchmarkConfig,
    baseline: &Path,
    round_dir: &Path,
    cancelled: &Arc<AtomicBool>,
) -> Result<RoundResult, BenchmarkError> {
    fs::create_dir_all(round_dir)?;
    let wal_path = round_dir.join("kv.wal");
    fs::copy(baseline, &wal_path)?;

    let address = available_local_address()?;
    let stdout = File::create(round_dir.join("server.stdout.log"))?;
    let stderr_path = round_dir.join("server.stderr.log");
    let stderr = File::create(&stderr_path)?;
    let mut command = Command::new(&config.server_executable);
    command
        .arg("--runtime")
        .arg(runtime_arg(&config.runtime))
        .arg("--lock")
        .arg(lock_arg(&config.lock))
        .arg("--bind")
        .arg(address.to_string())
        .arg("--data")
        .arg(&wal_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));

    let child = command.spawn().map_err(|error| {
        BenchmarkError::failed(format!(
            "failed to start {}: {error}",
            config.server_executable.display()
        ))
    })?;
    let mut server = ChildGuard::new(child);
    wait_until_ready(&mut server, address, &stderr_path, cancelled)?;

    let result = execute_clients(config, address, cancelled)?;
    if let Some(status) = server.try_wait()? {
        return Err(BenchmarkError::failed(format!(
            "kv-server exited during benchmark with {status}: {}",
            read_short_log(&stderr_path)
        )));
    }
    server.terminate()?;
    Ok(result)
}

fn execute_clients(
    config: &BenchmarkConfig,
    address: SocketAddr,
    cancelled: &Arc<AtomicBool>,
) -> Result<RoundResult, BenchmarkError> {
    let mut streams = Vec::with_capacity(config.clients);
    for _ in 0..config.clients {
        let stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        streams.push(stream);
    }

    let barrier = Arc::new(Barrier::new(config.clients + 1));
    let mut workers = Vec::with_capacity(config.clients);
    let mut first_request = 0_usize;
    for (client, stream) in streams.into_iter().enumerate() {
        let count = requests_for_client(config.requests, config.clients, client);
        let start_index = first_request;
        first_request += count;
        let barrier = Arc::clone(&barrier);
        let cancelled = Arc::clone(cancelled);
        let workload = config.workload;
        let dataset_keys = config.dataset_keys;
        let value_bytes = config.value_bytes;
        let seed = config.seed;
        workers.push(thread::spawn(move || {
            barrier.wait();
            run_client(
                stream,
                start_index,
                count,
                workload,
                dataset_keys,
                value_bytes,
                seed,
                &cancelled,
            )
        }));
    }

    let started = Instant::now();
    barrier.wait();
    let mut aggregate = WorkerResult::default();
    for worker in workers {
        let result = worker
            .join()
            .map_err(|_| BenchmarkError::failed("benchmark client thread panicked"))?;
        aggregate.merge(result);
    }
    let elapsed = started.elapsed();

    Ok(RoundResult {
        requested: u64::try_from(config.requests)
            .map_err(|_| BenchmarkError::failed("request count overflow"))?,
        attempted: aggregate.attempted,
        completed: aggregate.completed,
        success: aggregate.success,
        failed: aggregate.failed,
        elapsed,
        latencies_ns: aggregate.latencies_ns,
        cancelled: aggregate.cancelled || cancelled.load(Ordering::Relaxed),
        clients: config.clients,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_client(
    mut stream: TcpStream,
    first_request: usize,
    request_count: usize,
    workload: BenchmarkWorkload,
    dataset_keys: usize,
    value_bytes: usize,
    seed: u64,
    cancelled: &AtomicBool,
) -> WorkerResult {
    let reader_stream = match stream.try_clone() {
        Ok(stream) => stream,
        Err(_) => {
            return WorkerResult {
                failed: 1,
                ..WorkerResult::default()
            };
        }
    };
    let mut reader = BufReader::new(reader_stream);
    let mut result = WorkerResult::default();

    for request_index in first_request..first_request + request_count {
        if cancelled.load(Ordering::Relaxed) {
            result.cancelled = true;
            break;
        }

        let request = request_for(request_index, workload, dataset_keys, value_bytes, seed);
        let encoded = match encode_request_line(&request) {
            Ok(encoded) => encoded,
            Err(_) => {
                result.failed += 1;
                break;
            }
        };

        result.attempted += 1;
        let started = Instant::now();
        if stream.write_all(&encoded).is_err() {
            result.failed += 1;
            break;
        }

        let response = match read_frame(&mut reader) {
            Ok(Frame::Line(line)) => parse_response_bytes(&line),
            _ => {
                result.failed += 1;
                break;
            }
        };
        let latency = started.elapsed();
        result.latencies_ns.push(duration_to_nanos(latency));
        result.completed += 1;

        match response {
            Ok(response) if response.ok => result.success += 1,
            Ok(_) | Err(_) => result.failed += 1,
        }
    }

    result
}

fn request_for(
    request_index: usize,
    workload: BenchmarkWorkload,
    dataset_keys: usize,
    value_bytes: usize,
    seed: u64,
) -> Request {
    let random = splitmix64(seed ^ request_index as u64);
    let key_index = (splitmix64(random) % dataset_keys as u64) as usize;
    let key = benchmark_key(key_index);
    if random % 100 < u64::from(workload.write_percent()) {
        Request::Set {
            key,
            value: deterministic_value(value_bytes, random),
        }
    } else {
        Request::Get { key }
    }
}

fn summarize(
    config: &BenchmarkConfig,
    artifact_dir: &Path,
    points: Vec<BenchmarkPoint>,
    mut latencies_ns: Vec<u64>,
) -> Result<BenchmarkOutcome, BenchmarkError> {
    let requested = checked_sum(points.iter().map(|point| point.requested))?;
    let attempted = checked_sum(points.iter().map(|point| point.attempted))?;
    let completed = checked_sum(points.iter().map(|point| point.completed))?;
    let success = checked_sum(points.iter().map(|point| point.success))?;
    let failed = checked_sum(points.iter().map(|point| point.failed))?;
    let elapsed_ms = points.iter().map(|point| point.elapsed_ms).sum::<f64>();
    latencies_ns.sort_unstable();

    Ok(BenchmarkOutcome {
        artifact_dir: artifact_dir.to_path_buf(),
        runtime: runtime_arg(&config.runtime).to_owned(),
        lock: lock_arg(&config.lock).to_owned(),
        workload: config.workload,
        clients: config.clients,
        requests_per_run: config.requests,
        measured_runs: points,
        requested,
        attempted,
        completed,
        success,
        failed,
        elapsed_ms,
        throughput_qps: qps(completed, Duration::from_secs_f64(elapsed_ms / 1_000.0)),
        p50_ms: percentile_ms_sorted(&latencies_ns, 50),
        p95_ms: percentile_ms_sorted(&latencies_ns, 95),
        p99_ms: percentile_ms_sorted(&latencies_ns, 99),
    })
}

fn checked_sum(mut values: impl Iterator<Item = u64>) -> Result<u64, BenchmarkError> {
    values.try_fold(0_u64, |sum, value| {
        sum.checked_add(value)
            .ok_or_else(|| BenchmarkError::failed("benchmark counter overflow"))
    })
}

impl RoundResult {
    fn point(&self, run: usize) -> BenchmarkPoint {
        let mut latencies = self.latencies_ns.clone();
        latencies.sort_unstable();
        BenchmarkPoint {
            run,
            clients: self.clients,
            requested: self.requested,
            attempted: self.attempted,
            completed: self.completed,
            success: self.success,
            failed: self.failed,
            elapsed_ms: self.elapsed.as_secs_f64() * 1_000.0,
            throughput_qps: qps(self.completed, self.elapsed),
            p50_ms: percentile_ms_sorted(&latencies, 50),
            p95_ms: percentile_ms_sorted(&latencies, 95),
            p99_ms: percentile_ms_sorted(&latencies, 99),
        }
    }
}

fn qps(completed: u64, elapsed: Duration) -> f64 {
    let seconds = elapsed.as_secs_f64();
    if completed == 0 || seconds == 0.0 {
        0.0
    } else {
        completed as f64 / seconds
    }
}

fn percentile_ms_sorted(sorted_ns: &[u64], percentile: usize) -> f64 {
    if sorted_ns.is_empty() {
        return 0.0;
    }
    let rank = percentile
        .saturating_mul(sorted_ns.len())
        .div_ceil(100)
        .saturating_sub(1)
        .min(sorted_ns.len() - 1);
    sorted_ns[rank] as f64 / 1_000_000.0
}

/// 写出与持久化模块相同的 CRC32 JSON Lines 基线。
#[doc(hidden)]
pub fn write_deterministic_baseline(
    path: impl AsRef<Path>,
    dataset_keys: usize,
    value_bytes: usize,
    seed: u64,
) -> Result<(), BenchmarkError> {
    if dataset_keys == 0 {
        return Err(BenchmarkError::failed(
            "dataset_keys must be greater than zero",
        ));
    }
    if value_bytes == 0 || value_bytes > MAX_VALUE_BYTES {
        return Err(BenchmarkError::failed(format!(
            "value_bytes must be between 1 and {MAX_VALUE_BYTES}"
        )));
    }
    let path = path.as_ref();
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    for index in 0..dataset_keys {
        let record = BaselineWalRecord::Set {
            key: benchmark_key(index),
            value: deterministic_value(value_bytes, splitmix64(seed ^ index as u64)),
        };
        let record_bytes = serde_json::to_vec(&record)?;
        let entry = BaselineWalEntry {
            record,
            crc32: format!("{:08X}", crc32fast::hash(&record_bytes)),
        };
        serde_json::to_writer(&mut writer, &entry)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

/// 供证据测试验证最近秩分位数实现。
#[doc(hidden)]
pub fn percentile_millis(samples_ns: &[u64], percentile: usize) -> f64 {
    let mut sorted = samples_ns.to_vec();
    sorted.sort_unstable();
    percentile_ms_sorted(&sorted, percentile.clamp(1, 100))
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum BaselineWalRecord {
    Set { key: String, value: String },
}

#[derive(Serialize)]
struct BaselineWalEntry {
    record: BaselineWalRecord,
    crc32: String,
}

#[derive(Default)]
struct WorkerResult {
    attempted: u64,
    completed: u64,
    success: u64,
    failed: u64,
    latencies_ns: Vec<u64>,
    cancelled: bool,
}

impl WorkerResult {
    fn merge(&mut self, other: Self) {
        self.attempted += other.attempted;
        self.completed += other.completed;
        self.success += other.success;
        self.failed += other.failed;
        self.latencies_ns.extend(other.latencies_ns);
        self.cancelled |= other.cancelled;
    }
}

struct RoundResult {
    requested: u64,
    attempted: u64,
    completed: u64,
    success: u64,
    failed: u64,
    elapsed: Duration,
    latencies_ns: Vec<u64>,
    cancelled: bool,
    clients: usize,
}

#[derive(Serialize)]
struct ConfigArtifact {
    server_executable: String,
    runtime: String,
    lock: String,
    workload: BenchmarkWorkload,
    read_percent: u8,
    write_percent: u8,
    clients: usize,
    requests: usize,
    dataset_keys: usize,
    value_bytes: usize,
    seed: u64,
    warmup_runs: usize,
    measured_runs: usize,
    network: &'static str,
    protocol: &'static str,
    persistence: &'static str,
}

impl ConfigArtifact {
    fn from_config(config: &BenchmarkConfig) -> Self {
        Self {
            server_executable: config.server_executable.display().to_string(),
            runtime: runtime_arg(&config.runtime).to_owned(),
            lock: lock_arg(&config.lock).to_owned(),
            workload: config.workload,
            read_percent: config.workload.read_percent(),
            write_percent: config.workload.write_percent(),
            clients: config.clients,
            requests: config.requests,
            dataset_keys: config.dataset_keys,
            value_bytes: config.value_bytes,
            seed: config.seed,
            warmup_runs: config.warmup_runs,
            measured_runs: config.measured_runs,
            network: "localhost TCP",
            protocol: "JSON Lines",
            persistence: "WAL + flush + sync_data",
        }
    }
}

#[derive(Serialize)]
struct EnvironmentArtifact {
    captured_at_unix_ms: u128,
    git_commit: String,
    git_dirty: Option<bool>,
    rustc: String,
    os: String,
    architecture: String,
    cpu: String,
}

#[derive(Serialize)]
struct FailureArtifact {
    cancelled: bool,
    message: String,
    captured_at_unix_ms: u128,
}

fn environment() -> EnvironmentArtifact {
    EnvironmentArtifact {
        captured_at_unix_ms: unix_time_millis(),
        git_commit: command_output(
            Command::new("git")
                .arg("-C")
                .arg(env!("CARGO_MANIFEST_DIR"))
                .args(["rev-parse", "HEAD"]),
        )
        .unwrap_or_else(|| "unknown".to_owned()),
        git_dirty: git_dirty(),
        rustc: command_output(Command::new("rustc").arg("--version"))
            .unwrap_or_else(|| "unknown".to_owned()),
        os: std::env::consts::OS.to_owned(),
        architecture: std::env::consts::ARCH.to_owned(),
        cpu: cpu_description(),
    }
}

fn git_dirty() -> Option<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(env!("CARGO_MANIFEST_DIR"))
        .args(["status", "--porcelain"])
        .output()
        .ok()?;
    output.status.success().then_some(!output.stdout.is_empty())
}

fn command_output(command: &mut Command) -> Option<String> {
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn cpu_description() -> String {
    if let Ok(cpu) = std::env::var("PROCESSOR_IDENTIFIER")
        && !cpu.trim().is_empty()
    {
        return cpu;
    }
    if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo")
        && let Some(model) = cpuinfo.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.trim() == "model name").then(|| value.trim().to_owned())
        })
    {
        return model;
    }
    "unknown".to_owned()
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), BenchmarkError> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

fn wait_until_ready(
    child: &mut ChildGuard,
    address: SocketAddr,
    stderr_path: &Path,
    cancelled: &AtomicBool,
) -> Result<(), BenchmarkError> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err(BenchmarkError::Cancelled {
                artifact_dir: PathBuf::new(),
            });
        }
        if let Some(status) = child.try_wait()? {
            return Err(BenchmarkError::failed(format!(
                "kv-server exited before it became ready with {status}: {}",
                read_short_log(stderr_path)
            )));
        }
        if ping_server(address).is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(BenchmarkError::failed(format!(
                "kv-server did not become ready within 10 seconds: {}",
                read_short_log(stderr_path)
            )));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn ping_server(address: SocketAddr) -> Result<(), BenchmarkError> {
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_millis(200))?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    stream.set_write_timeout(Some(Duration::from_millis(500)))?;
    let reader_stream = stream.try_clone()?;
    stream.write_all(
        &encode_request_line(&Request::Ping)
            .map_err(|error| BenchmarkError::failed(error.to_string()))?,
    )?;
    let mut reader = BufReader::new(reader_stream);
    let Frame::Line(line) = read_frame(&mut reader)? else {
        return Err(BenchmarkError::failed("invalid ping response frame"));
    };
    let response =
        parse_response_bytes(&line).map_err(|error| BenchmarkError::failed(error.to_string()))?;
    if response.ok {
        Ok(())
    } else {
        Err(BenchmarkError::failed("ping request failed"))
    }
}

struct ChildGuard {
    child: Child,
    terminated: bool,
}

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self {
            child,
            terminated: false,
        }
    }

    fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    fn terminate(&mut self) -> io::Result<()> {
        if self.child.try_wait()?.is_none() {
            self.child.kill()?;
        }
        self.child.wait()?;
        self.terminated = true;
        Ok(())
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.terminated {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct TemporaryWorkspace {
    path: PathBuf,
}

impl TemporaryWorkspace {
    fn new() -> Result<Self, BenchmarkError> {
        let root = std::env::temp_dir();
        Ok(Self {
            path: create_unique_directory(&root, "rust-kv-benchmark-work")?,
        })
    }
}

impl Drop for TemporaryWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn create_unique_directory(root: &Path, prefix: &str) -> Result<PathBuf, BenchmarkError> {
    for _ in 0..100 {
        let sequence = ARTIFACT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = root.join(format!(
            "{prefix}-{}-{}-{sequence}",
            unix_time_millis(),
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(BenchmarkError::failed(
        "failed to allocate a unique benchmark directory",
    ))
}

fn check_cancelled(cancelled: &AtomicBool, artifact_dir: &Path) -> Result<(), BenchmarkError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(BenchmarkError::Cancelled {
            artifact_dir: artifact_dir.to_path_buf(),
        })
    } else {
        Ok(())
    }
}

fn available_local_address() -> io::Result<SocketAddr> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    listener.local_addr()
}

fn requests_for_client(total: usize, clients: usize, client: usize) -> usize {
    total / clients + usize::from(client < total % clients)
}

fn benchmark_key(index: usize) -> String {
    format!("bench_{index:08}")
}

fn deterministic_value(length: usize, seed: u64) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut output = String::with_capacity(length);
    let mut state = seed;
    for _ in 0..length {
        state = splitmix64(state);
        output.push(ALPHABET[(state % ALPHABET.len() as u64) as usize] as char);
    }
    output
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn duration_to_nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

fn runtime_arg(runtime: &RuntimeMode) -> &'static str {
    match runtime {
        RuntimeMode::Sync => "sync",
        RuntimeMode::Async => "async",
    }
}

fn lock_arg(lock: &LockStrategy) -> &'static str {
    match lock {
        LockStrategy::Mutex => "mutex",
        LockStrategy::RwLock => "rwlock",
    }
}

fn default_server_executable() -> PathBuf {
    let executable = if cfg!(windows) {
        "kv-server.exe"
    } else {
        "kv-server"
    };
    PathBuf::from("target").join("release").join(executable)
}

fn read_short_log(path: &Path) -> String {
    let Ok(contents) = fs::read_to_string(path) else {
        return "no server log available".to_owned();
    };
    let trimmed = contents.trim();
    if trimmed.chars().count() <= 2_000 {
        trimmed.to_owned()
    } else {
        let reversed = trimmed.chars().rev().take(2_000).collect::<String>();
        reversed.chars().rev().collect()
    }
}

fn unix_time_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
