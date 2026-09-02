use std::{env, fs, path::Path, time::Instant};

use rust_kv_store::persistence::PersistentStore;
use tempfile::tempdir;

const DEFAULT_OPERATIONS: usize = 2_000;
const DEFAULT_LIVE_KEYS: usize = 100;
const RECOVERY_REPEATS: usize = 7;

fn parse_positive(index: usize, default: usize, name: &str) -> usize {
    match env::args().nth(index) {
        Some(value) => value
            .parse::<usize>()
            .ok()
            .filter(|number| *number > 0)
            .unwrap_or_else(|| panic!("{name}必须是正整数")),
        None => default,
    }
}

fn expected_value(operation_count: usize, key_index: usize, live_keys: usize) -> String {
    let last_operation = operation_count - 1;
    let distance = (last_operation - key_index) % live_keys;
    format!("value:{:08}", last_operation - distance)
}

fn verify_final_state(store: &PersistentStore, operations: usize, live_keys: usize) {
    let expected_entries = operations.min(live_keys);
    assert_eq!(store.len(), expected_entries);

    for key_index in 0..expected_entries {
        let key = format!("key:{key_index:04}");
        assert_eq!(
            store.get(&key).unwrap(),
            expected_value(operations, key_index, live_keys)
        );
    }
}

fn median_recovery_us(wal_path: &Path, operations: usize, live_keys: usize) -> u128 {
    let mut samples = Vec::with_capacity(RECOVERY_REPEATS);

    for _ in 0..RECOVERY_REPEATS {
        let started = Instant::now();
        let store = PersistentStore::open(wal_path).unwrap();
        let elapsed = started.elapsed().as_micros();
        verify_final_state(&store, operations, live_keys);
        samples.push(elapsed);
    }

    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn main() {
    let operations = parse_positive(1, DEFAULT_OPERATIONS, "operations");
    let live_keys = parse_positive(2, DEFAULT_LIVE_KEYS, "live_keys");
    let temp = tempdir().unwrap();
    let wal_path = temp.path().join("benchmark.wal");

    let mut store = PersistentStore::open(&wal_path).unwrap();
    let write_started = Instant::now();
    for operation in 0..operations {
        let key = format!("key:{:04}", operation % live_keys);
        let value = format!("value:{operation:08}");
        store.set(key, value).unwrap();
    }
    verify_final_state(&store, operations, live_keys);
    let write_us = write_started.elapsed().as_micros();

    let wal_bytes_before = fs::metadata(&wal_path).unwrap().len();
    drop(store);
    let recovery_before_compact_median_us = median_recovery_us(&wal_path, operations, live_keys);

    let mut store = PersistentStore::open(&wal_path).unwrap();
    let compact_started = Instant::now();
    let compact = store.compact().unwrap();
    let compact_us = compact_started.elapsed().as_micros();
    let snapshot_bytes = compact.snapshot_bytes;
    drop(store);

    let wal_bytes_after = fs::metadata(&wal_path).unwrap().len();
    let disk_bytes_after = snapshot_bytes + wal_bytes_after;
    let recovery_after_compact_median_us = median_recovery_us(&wal_path, operations, live_keys);

    println!("variant=snapshot-compaction");
    println!("operations={operations}");
    println!("live_keys={}", operations.min(live_keys));
    println!("wal_bytes_before={wal_bytes_before}");
    println!("wal_bytes_after={wal_bytes_after}");
    println!("snapshot_bytes={snapshot_bytes}");
    println!("disk_bytes_after={disk_bytes_after}");
    println!("write_us={write_us}");
    println!("compact_us={compact_us}");
    println!("recovery_before_compact_median_us={recovery_before_compact_median_us}");
    println!("recovery_after_compact_median_us={recovery_after_compact_median_us}");
}
