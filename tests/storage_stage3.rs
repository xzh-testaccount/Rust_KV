use rust_kv_store::{
    error::ErrorCode,
    storage::{SetOutcome, Store},
};

#[test]
fn complete_crud_flow_works_without_network() {
    let mut store = Store::new();

    assert_eq!(store.set("course", "Rust").unwrap(), SetOutcome::Created);
    assert_eq!(store.set("name", "Alice").unwrap(), SetOutcome::Created);
    assert_eq!(
        store.set("course", "Advanced Rust").unwrap(),
        SetOutcome::Replaced {
            previous: "Rust".to_owned(),
        }
    );
    assert_eq!(store.get("course").unwrap(), "Advanced Rust");
    assert_eq!(store.keys(), vec!["course", "name"]);
    assert_eq!(store.stats().entries, 2);
    assert_eq!(store.delete("name").unwrap(), "Alice");
    assert_eq!(store.len(), 1);
}

#[test]
fn invalid_input_has_stable_error_codes() {
    let mut store = Store::new();

    let invalid_key = store.set("two words", "value").unwrap_err();
    assert_eq!(invalid_key.code(), ErrorCode::InvalidKey);
    assert_eq!(invalid_key.client_message(), "键不能包含空白字符");

    let invalid_value = store.set("key", "line\nbreak").unwrap_err();
    assert_eq!(invalid_value.code(), ErrorCode::InvalidValue);
    assert_eq!(invalid_value.client_message(), "值不能包含控制字符");
}

#[test]
fn missing_key_has_a_clear_message() {
    let store = Store::new();

    let error = store.get("missing").unwrap_err();
    assert_eq!(error.code(), ErrorCode::NotFound);
    assert_eq!(error.client_message(), "键不存在：missing");
}
