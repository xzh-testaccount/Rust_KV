use std::{env, fs, path::Path, time::Instant};

use rust_kv_store::{
    persistence::PersistentStore as AdvancedStore, persistence_basic::PersistentStore as BasicStore,
};
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

fn verify_basic(store: &BasicStore, operations: usize, live_keys: usize) {
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

fn verify_advanced(store: &AdvancedStore, operations: usize, live_keys: usize) {
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

fn median_basic_recovery_us(path: &Path, operations: usize, live_keys: usize) -> u128 {
    let mut samples = Vec::with_capacity(RECOVERY_REPEATS);
    for _ in 0..RECOVERY_REPEATS {
        let started = Instant::now();
        let store = BasicStore::open(path).unwrap();
        let elapsed = started.elapsed().as_micros();
        verify_basic(&store, operations, live_keys);
        samples.push(elapsed);
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn median_advanced_recovery_us(path: &Path, operations: usize, live_keys: usize) -> u128 {
    let mut samples = Vec::with_capacity(RECOVERY_REPEATS);
    for _ in 0..RECOVERY_REPEATS {
        let started = Instant::now();
        let store = AdvancedStore::open(path).unwrap();
        let elapsed = started.elapsed().as_micros();
        verify_advanced(&store, operations, live_keys);
        samples.push(elapsed);
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn main() {
    let operations = parse_positive(1, DEFAULT_OPERATIONS, "operations");
    let live_keys = parse_positive(2, DEFAULT_LIVE_KEYS, "live_keys");
    let temp = tempdir().unwrap();
    let basic_wal = temp.path().join("basic.wal");
    let advanced_wal = temp.path().join("advanced.wal");

    let mut basic = BasicStore::open(&basic_wal).unwrap();
    let basic_write_started = Instant::now();
    for operation in 0..operations {
        basic
            .set(
                format!("key:{:04}", operation % live_keys),
                format!("value:{operation:08}"),
            )
            .unwrap();
    }
    let basic_write_us = basic_write_started.elapsed().as_micros();
    verify_basic(&basic, operations, live_keys);
    drop(basic);
    let basic_disk_bytes = fs::metadata(&basic_wal).unwrap().len();
    let basic_recovery_us = median_basic_recovery_us(&basic_wal, operations, live_keys);

    let mut advanced = AdvancedStore::open(&advanced_wal).unwrap();
    let advanced_write_started = Instant::now();
    for operation in 0..operations {
        advanced
            .set(
                format!("key:{:04}", operation % live_keys),
                format!("value:{operation:08}"),
            )
            .unwrap();
    }
    let advanced_write_us = advanced_write_started.elapsed().as_micros();
    verify_advanced(&advanced, operations, live_keys);
    drop(advanced);
    let advanced_disk_before_bytes = fs::metadata(&advanced_wal).unwrap().len();
    let advanced_recovery_before_us =
        median_advanced_recovery_us(&advanced_wal, operations, live_keys);

    let mut advanced = AdvancedStore::open(&advanced_wal).unwrap();
    let compact_started = Instant::now();
    let compact = advanced.compact().unwrap();
    let compact_us = compact_started.elapsed().as_micros();
    drop(advanced);
    let advanced_disk_after_bytes =
        fs::metadata(&advanced_wal).unwrap().len() + compact.snapshot_bytes;
    let advanced_recovery_after_us =
        median_advanced_recovery_us(&advanced_wal, operations, live_keys);

    println!("operations={operations}");
    println!("live_keys={}", operations.min(live_keys));
    println!("basic_write_us={basic_write_us}");
    println!("advanced_write_us={advanced_write_us}");
    println!("basic_disk_bytes={basic_disk_bytes}");
    println!("advanced_disk_before_bytes={advanced_disk_before_bytes}");
    println!("advanced_disk_after_bytes={advanced_disk_after_bytes}");
    println!("compact_us={compact_us}");
    println!("basic_recovery_us={basic_recovery_us}");
    println!("advanced_recovery_before_us={advanced_recovery_before_us}");
    println!("advanced_recovery_after_us={advanced_recovery_after_us}");
}
