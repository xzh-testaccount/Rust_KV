use std::{
    fs,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

use rust_kv_store::{
    benchmark::{
        BenchmarkConfig, BenchmarkPhase, BenchmarkWorkload, percentile_millis, run_benchmark,
        write_deterministic_baseline,
    },
    persistence::PersistentStore,
    server::{LockStrategy, RuntimeMode},
};
use tempfile::tempdir;

#[test]
fn deterministic_baseline_matches_the_crc32_wal_format() {
    let temp = tempdir().unwrap();
    let first = temp.path().join("first.wal");
    let second = temp.path().join("second.wal");

    write_deterministic_baseline(&first, 12, 32, 7).unwrap();
    write_deterministic_baseline(&second, 12, 32, 7).unwrap();

    assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
    let store = PersistentStore::open(&first).unwrap();
    assert_eq!(store.len(), 12);
    assert_eq!(store.get("bench_00000000").unwrap().len(), 32);
    assert_eq!(store.get("bench_00000011").unwrap().len(), 32);
}

#[test]
fn nearest_rank_percentiles_use_real_samples() {
    let samples = [1_000_000, 2_000_000, 3_000_000, 4_000_000, 100_000_000];
    assert_eq!(percentile_millis(&samples, 50), 3.0);
    assert_eq!(percentile_millis(&samples, 95), 100.0);
    assert_eq!(percentile_millis(&samples, 99), 100.0);
}

#[test]
fn real_server_run_creates_a_traceable_evidence_bundle() {
    let temp = tempdir().unwrap();
    let config = BenchmarkConfig {
        server_executable: PathBuf::from(env!("CARGO_BIN_EXE_kv-server")),
        artifact_root: temp.path().join("artifacts"),
        runtime: RuntimeMode::Async,
        lock: LockStrategy::Mutex,
        workload: BenchmarkWorkload::Mixed,
        clients: 2,
        requests: 20,
        dataset_keys: 8,
        value_bytes: 16,
        seed: 42,
        warmup_runs: 0,
        measured_runs: 1,
    };
    let mut progress = Vec::new();

    let outcome = run_benchmark(config, Arc::new(AtomicBool::new(false)), |event| {
        progress.push(event.phase);
    })
    .unwrap();

    assert_eq!(outcome.requested, 20);
    assert_eq!(outcome.attempted, 20);
    assert_eq!(outcome.completed, 20);
    assert_eq!(outcome.success, 20);
    assert_eq!(outcome.failed, 0);
    assert!(outcome.throughput_qps > 0.0);
    assert_eq!(outcome.measured_runs.len(), 1);
    assert!(progress.contains(&BenchmarkPhase::Preparing));
    assert!(progress.contains(&BenchmarkPhase::Measured));
    assert!(progress.contains(&BenchmarkPhase::Completed));

    assert!(outcome.artifact_dir.join("environment.json").is_file());
    assert!(outcome.artifact_dir.join("config.json").is_file());
    assert!(
        outcome
            .artifact_dir
            .join("raw")
            .join("run-01.json")
            .is_file()
    );
    assert!(outcome.artifact_dir.join("summary.json").is_file());

    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(outcome.artifact_dir.join("summary.json")).unwrap())
            .unwrap();
    assert_eq!(summary["success"], 20);
    assert_eq!(summary["failed"], 0);
    assert_eq!(summary["runtime"], "async");
    assert_eq!(summary["lock"], "mutex");
}
