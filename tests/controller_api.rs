use std::{
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use tempfile::tempdir;

struct ControllerGuard {
    child: Child,
    address: SocketAddr,
}

impl Drop for ControllerGuard {
    fn drop(&mut self) {
        let _ = http_json(self.address, "POST", "/api/server/kill", None);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn controller_drives_real_crud_concurrency_recovery_and_benchmark() {
    let temp = tempdir().unwrap();
    let controller_address = free_address();
    let server_address = free_address();
    let wal_path = temp.path().join("main.wal");
    let artifact_root = temp.path().join("artifacts");
    let child = Command::new(env!("CARGO_BIN_EXE_kv-controller"))
        .arg("--bind")
        .arg(controller_address.to_string())
        .arg("--server-bind")
        .arg(server_address.to_string())
        .arg("--data")
        .arg(&wal_path)
        .arg("--server-bin")
        .arg(env!("CARGO_BIN_EXE_kv-server"))
        .arg("--artifacts")
        .arg(&artifact_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut controller = ControllerGuard {
        child,
        address: controller_address,
    };

    wait_for(Duration::from_secs(10), || {
        http_json(controller_address, "GET", "/api/server/state", None)
            .ok()
            .is_some_and(|value| value["state"] == "ONLINE")
    });

    let set = http_json(
        controller_address,
        "POST",
        "/api/kv",
        Some(json!({"cmd":"set","key":"course","value":"Rust"})),
    )
    .unwrap();
    assert_eq!(set["ok"], true);
    let get = http_json(
        controller_address,
        "POST",
        "/api/kv",
        Some(json!({"cmd":"get","key":"course"})),
    )
    .unwrap();
    assert_eq!(get["data"]["value"], "Rust");

    let storage = http_json(controller_address, "GET", "/api/storage/state", None).unwrap();
    assert_eq!(storage["engine"], "snapshot-wal-v1");
    assert_eq!(storage["entries"], 1);
    assert_eq!(storage["walRecords"], 1);
    assert!(storage["walBytes"].as_u64().unwrap() > 0);
    assert_eq!(storage["snapshotBytes"], 0);
    assert_eq!(storage["lastSequence"], 1);
    assert_eq!(storage["writable"], true);

    let compacted = http_json(
        controller_address,
        "POST",
        "/api/storage/compact",
        Some(json!({})),
    )
    .unwrap();
    assert_eq!(compacted["compacted"], true);
    assert_eq!(compacted["before"]["entries"], 1);
    assert_eq!(compacted["before"]["walRecords"], 1);
    assert!(compacted["before"]["walBytes"].as_u64().unwrap() > 0);
    assert_eq!(compacted["after"]["entries"], 1);
    assert_eq!(compacted["after"]["walRecords"], 0);
    assert_eq!(compacted["after"]["walBytes"], 0);
    assert!(compacted["after"]["snapshotBytes"].as_u64().unwrap() > 0);
    assert_eq!(compacted["after"]["lastSequence"], 1);
    assert!(wal_path.with_extension("snapshot").is_file());

    let storage = http_json(controller_address, "GET", "/api/storage/state", None).unwrap();
    assert_eq!(storage["entries"], 1);
    assert_eq!(storage["walRecords"], 0);
    assert_eq!(storage["walBytes"], 0);
    assert!(storage["snapshotBytes"].as_u64().unwrap() > 0);
    assert_eq!(storage["totalBytes"], storage["snapshotBytes"]);

    let started = http_json(
        controller_address,
        "POST",
        "/api/experiment/start",
        Some(json!({"clients":4,"requestsPerClient":10,"workload":"mixed"})),
    )
    .unwrap();
    assert_eq!(started["accepted"], true);
    wait_for(Duration::from_secs(15), || {
        http_json(controller_address, "GET", "/api/experiment/state", None)
            .ok()
            .is_some_and(|value| value["status"] == "COMPLETED")
    });
    let concurrency = http_json(controller_address, "GET", "/api/experiment/state", None).unwrap();
    assert_eq!(concurrency["successful"], 40);
    assert_eq!(concurrency["failed"], 0);

    let prepared = http_json(
        controller_address,
        "POST",
        "/api/recovery/prepare",
        Some(json!({"count":5})),
    )
    .unwrap();
    assert_eq!(prepared["phase"], "PREPARED");
    let before_count = prepared["before"]["count"].as_u64().unwrap();
    let before_fingerprint = prepared["before"]["fingerprint"].clone();

    let crashed = http_json(controller_address, "POST", "/api/recovery/kill", None).unwrap();
    assert_eq!(crashed["phase"], "CRASHED");
    let restarting = http_json(controller_address, "POST", "/api/recovery/restart", None).unwrap();
    assert_eq!(restarting["phase"], "RECOVERING");
    wait_for(Duration::from_secs(15), || {
        http_json(controller_address, "GET", "/api/recovery/state", None)
            .ok()
            .is_some_and(|value| value["phase"] == "VERIFIED")
    });
    let recovered = http_json(controller_address, "GET", "/api/recovery/state", None).unwrap();
    assert_eq!(recovered["verified"], true);
    assert_eq!(recovered["lost"], 0);
    assert_eq!(recovered["after"]["count"], before_count);
    assert_eq!(recovered["after"]["fingerprint"], before_fingerprint);

    let capabilities = http_json(
        controller_address,
        "GET",
        "/api/benchmark/capabilities",
        None,
    )
    .unwrap();
    assert_eq!(capabilities["profiles"][0]["id"], "quick");
    assert_eq!(capabilities["profiles"][0]["scales"], json!([1, 128]));
    assert_eq!(capabilities["profiles"][0]["sampling"], "fixed_duration");
    assert_eq!(capabilities["profiles"][0]["sampleDurationMs"], 3_000);
    assert_eq!(capabilities["profiles"][1]["id"], "full");
    assert_eq!(
        capabilities["profiles"][1]["scales"],
        json!([1, 10, 50, 100])
    );
    assert_eq!(capabilities["profiles"][1]["sampling"], "fixed_requests");
    assert_eq!(capabilities["profiles"][1]["requestsPerScale"], 10_000);
    assert_eq!(capabilities["profiles"][1]["warmupRuns"], 1);
    assert_eq!(capabilities["profiles"][1]["measuredRuns"], 5);
    assert_eq!(capabilities["fixedConditions"]["datasetKeys"], 10_000);
    assert_eq!(capabilities["fixedConditions"]["valueBytes"], 128);
    assert_eq!(
        capabilities["fixedConditions"]["persistence"],
        "WAL + flush + sync_data"
    );
    assert_eq!(capabilities["fixedConditions"]["protocol"], "JSON Lines");
    assert_eq!(capabilities["fixedConditions"]["network"], "localhost");
    assert_eq!(capabilities["fixedConditions"]["seed"], 20_260_902);

    let reset = http_json_segmented(
        controller_address,
        "POST",
        "/api/benchmark/reset",
        Some(json!({})),
    )
    .unwrap();
    assert_eq!(reset["reset"], true);
    assert_eq!(reset["ready"], true);
    assert_eq!(reset["resetEpoch"], 1);
    let benchmark_started = http_json(
        controller_address,
        "POST",
        "/api/benchmark/start",
        Some(json!({
            "scales":[2],
            "requestsPerScale":20,
            "runtime":"async",
            "lock":"rwlock",
            "workload":"read"
        })),
    )
    .unwrap();
    assert_eq!(benchmark_started["accepted"], true);
    wait_for(Duration::from_secs(40), || {
        http_json(controller_address, "GET", "/api/benchmark/state", None)
            .ok()
            .is_some_and(|value| value["status"] == "COMPLETED")
    });
    let benchmark = http_json(controller_address, "GET", "/api/benchmark/state", None).unwrap();
    assert_eq!(benchmark["points"].as_array().unwrap().len(), 1);
    assert!(benchmark["points"][0]["qps"].as_f64().unwrap() > 0.0);
    let artifact_dir = PathBuf::from(benchmark["artifactDir"].as_str().unwrap());
    assert!(artifact_dir.join("environment.json").is_file());
    assert!(artifact_dir.join("config.json").is_file());
    assert!(artifact_dir.join("raw/run-05.json").is_file());
    assert!(artifact_dir.join("summary.json").is_file());

    let quick_reset = http_json(
        controller_address,
        "POST",
        "/api/benchmark/reset",
        Some(json!({})),
    )
    .unwrap();
    assert_eq!(quick_reset["resetEpoch"], 2);
    let quick_started = http_json(
        controller_address,
        "POST",
        "/api/benchmark/start",
        Some(json!({
            "benchmarkProfile":"quick",
            "scales":[1,128],
            "runtime":"async",
            "lock":"rwlock",
            "workload":"read"
        })),
    )
    .unwrap();
    assert_eq!(quick_started["accepted"], true);
    wait_for(Duration::from_secs(15), || {
        http_json(controller_address, "GET", "/api/benchmark/state", None)
            .ok()
            .is_some_and(|value| {
                value["status"] == "RUNNING"
                    && value["points"]
                        .as_array()
                        .is_some_and(|points| points.len() == 1)
            })
    });
    wait_for(Duration::from_secs(20), || {
        http_json(controller_address, "GET", "/api/benchmark/state", None)
            .ok()
            .is_some_and(|value| value["status"] == "COMPLETED")
    });
    let quick = http_json(controller_address, "GET", "/api/benchmark/state", None).unwrap();
    assert_eq!(quick["mode"], "quick");
    assert_eq!(quick["sampling"], "fixed_duration");
    assert_eq!(quick["sampleDurationMs"], 3_000);
    assert_eq!(quick["resetEpoch"], 2);
    assert!(quick["startedAtUnixMs"].as_u64().unwrap() > 0);
    assert!(
        quick["completedAtUnixMs"].as_u64().unwrap() >= quick["startedAtUnixMs"].as_u64().unwrap()
    );
    assert_eq!(quick["points"].as_array().unwrap().len(), 2);
    assert_eq!(quick["points"][0]["clients"], 1);
    assert_eq!(quick["points"][1]["clients"], 128);
    for point in quick["points"].as_array().unwrap() {
        let elapsed_ms = point["elapsedMs"].as_f64().unwrap();
        assert!((2_500.0..=7_000.0).contains(&elapsed_ms));
    }
    assert!(quick["points"][1]["success"].as_u64().unwrap() > 0);
    assert_eq!(quick["points"][1]["failed"], 0);

    http_json(controller_address, "POST", "/api/server/kill", None).unwrap();
    controller.child.kill().unwrap();
    controller.child.wait().unwrap();
}

#[test]
fn benchmark_failure_is_closed_without_publishing_a_point() {
    let temp = tempdir().unwrap();
    let controller_address = free_address();
    let server_address = free_address();
    let child = Command::new(env!("CARGO_BIN_EXE_kv-controller"))
        .arg("--bind")
        .arg(controller_address.to_string())
        .arg("--server-bind")
        .arg(server_address.to_string())
        .arg("--data")
        .arg(temp.path().join("main.wal"))
        .arg("--server-bin")
        .arg(temp.path().join("missing-kv-server.exe"))
        .arg("--artifacts")
        .arg(temp.path().join("artifacts"))
        .arg("--no-auto-start")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut controller = ControllerGuard {
        child,
        address: controller_address,
    };

    wait_for(Duration::from_secs(10), || {
        http_json(controller_address, "GET", "/api/server/state", None).is_ok()
    });
    http_json(
        controller_address,
        "POST",
        "/api/benchmark/reset",
        Some(json!({})),
    )
    .unwrap();
    let started = http_json(
        controller_address,
        "POST",
        "/api/benchmark/start",
        Some(json!({
            "benchmarkProfile":"quick",
            "scales":[1],
            "runtime":"async",
            "lock":"mutex",
            "workload":"read"
        })),
    )
    .unwrap();
    assert_eq!(started["accepted"], true);
    wait_for(Duration::from_secs(10), || {
        http_json(controller_address, "GET", "/api/benchmark/state", None)
            .ok()
            .is_some_and(|value| value["status"] == "INTERRUPTED")
    });
    let state = http_json(controller_address, "GET", "/api/benchmark/state", None).unwrap();
    assert_eq!(state["status"], "INTERRUPTED");
    assert!(
        state["error"]
            .as_str()
            .is_some_and(|error| !error.is_empty())
    );
    assert!(state["points"].as_array().unwrap().is_empty());

    controller.child.kill().unwrap();
    controller.child.wait().unwrap();
}

fn free_address() -> SocketAddr {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
}

fn wait_for(timeout: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(40));
    }
    panic!("condition did not become true within {timeout:?}");
}

fn http_json(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> std::io::Result<Value> {
    let body = body.map(|value| value.to_string()).unwrap_or_default();
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()?;
    read_json_response(&mut stream)
}

fn http_json_segmented(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> std::io::Result<Value> {
    let body = body.map(|value| value.to_string()).unwrap_or_default();
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: {address}\r\n")?;
    stream.flush()?;
    thread::sleep(Duration::from_millis(20));
    write!(
        stream,
        "Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.flush()?;
    thread::sleep(Duration::from_millis(20));
    stream.write_all(body.as_bytes())?;
    stream.flush()?;
    read_json_response(&mut stream)
}

fn read_json_response(stream: &mut TcpStream) -> std::io::Result<Value> {
    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| std::io::Error::other("missing HTTP header boundary"))?;
    serde_json::from_slice(&response[split + 4..]).map_err(std::io::Error::other)
}
