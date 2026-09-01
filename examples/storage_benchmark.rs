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

    let write_started = Instant::now();
    {
        let mut store = PersistentStore::open(&wal_path).unwrap();
        for operation in 0..operations {
            let key = format!("key:{:04}", operation % live_keys);
            let value = format!("value:{operation:08}");
            store.set(key, value).unwrap();
        }
        verify_final_state(&store, operations, live_keys);
    }
    let write_ms = write_started.elapsed().as_millis();

    let wal_bytes = fs::metadata(&wal_path).unwrap().len();
    let recovery_median_us = median_recovery_us(&wal_path, operations, live_keys);

    println!("variant=basic");
    println!("operations={operations}");
    println!("live_keys={}", operations.min(live_keys));
    println!("wal_bytes={wal_bytes}");
    println!("write_ms={write_ms}");
    println!("recovery_median_us={recovery_median_us}");
}
