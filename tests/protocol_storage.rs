//! Offline protocol and storage tests: these protect the business rules before networking exists.

use rust_kv_store::error::{AppError, ErrorCode};
use rust_kv_store::protocol::{
    MAX_FRAME_BYTES, MAX_KEY_BYTES, MAX_VALUE_BYTES, Request, Response, ResponseData,
    encode_request_line, encode_response_line, parse_command, parse_request_bytes,
    parse_request_line, parse_response_bytes, parse_response_line,
};
use rust_kv_store::storage::Store;

fn code(error: AppError) -> ErrorCode {
    error.code()
}

#[test]
fn all_public_error_codes_keep_the_wire_names_and_trigger_paths() {
    let names = [
        (ErrorCode::InvalidUtf8, "\"INVALID_UTF8\""),
        (ErrorCode::InvalidJson, "\"INVALID_JSON\""),
        (ErrorCode::InvalidRequest, "\"INVALID_REQUEST\""),
        (ErrorCode::UnknownCommand, "\"UNKNOWN_COMMAND\""),
        (ErrorCode::MissingArgument, "\"MISSING_ARGUMENT\""),
        (ErrorCode::ExtraArgument, "\"EXTRA_ARGUMENT\""),
        (ErrorCode::InvalidKey, "\"INVALID_KEY\""),
        (ErrorCode::InvalidValue, "\"INVALID_VALUE\""),
        (ErrorCode::NotFound, "\"NOT_FOUND\""),
        (ErrorCode::FrameTooLarge, "\"FRAME_TOO_LARGE\""),
        (ErrorCode::StorageError, "\"STORAGE_ERROR\""),
    ];
    for (error_code, wire_name) in names {
        assert_eq!(
            serde_json::to_string(&error_code).expect("serialize error code"),
            wire_name
        );
        assert_eq!(error_code.to_string(), wire_name.trim_matches('"'));
    }

    assert_eq!(
        code(parse_request_bytes(b"{\xff}\n").expect_err("invalid UTF-8")),
        ErrorCode::InvalidUtf8
    );
    assert_eq!(
        code(parse_request_line("not JSON\n").expect_err("invalid JSON")),
        ErrorCode::InvalidJson
    );
    assert_eq!(
        code(parse_request_line("[]\n").expect_err("non-object request")),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(parse_command("missing-command").expect_err("unknown command")),
        ErrorCode::UnknownCommand
    );
    assert_eq!(
        code(parse_command("get").expect_err("missing CLI argument")),
        ErrorCode::MissingArgument
    );
    assert_eq!(
        code(parse_command("get key extra").expect_err("extra CLI argument")),
        ErrorCode::ExtraArgument
    );
    let mut store = Store::new();
    assert_eq!(
        code(store.get("bad key").expect_err("invalid key")),
        ErrorCode::InvalidKey
    );
    assert_eq!(
        code(store.set("key", "").expect_err("invalid value")),
        ErrorCode::InvalidValue
    );
    assert_eq!(
        code(store.get("missing").expect_err("missing key")),
        ErrorCode::NotFound
    );
    let oversized_request = format!(
        r#"{{"cmd":"set","key":"k","value":"{}"}}"#,
        "x".repeat(MAX_FRAME_BYTES)
    );
    assert_eq!(
        code(parse_request_line(&format!("{oversized_request}\n")).expect_err("oversized frame")),
        ErrorCode::FrameTooLarge
    );
    assert_eq!(
        code(AppError::Io(std::io::Error::other("disk failure"))),
        ErrorCode::StorageError
    );
}

#[test]
fn every_cli_command_maps_to_a_wire_request() {
    let cases = [
        (
            "set answer 42",
            Request::Set {
                key: "answer".into(),
                value: "42".into(),
            },
        ),
        (
            "get answer",
            Request::Get {
                key: "answer".into(),
            },
        ),
        (
            "delete answer",
            Request::Delete {
                key: "answer".into(),
            },
        ),
        ("keys", Request::Keys),
        ("status", Request::Status),
        ("ping", Request::Ping),
        ("quit", Request::Quit),
    ];

    for (command, expected) in cases {
        assert_eq!(parse_command(command).expect("valid CLI command"), expected);
    }
}

#[test]
fn quoted_cli_value_preserves_spaces_without_relaxing_argument_rules() {
    let request = parse_command(r#"set greeting "hello world""#).expect("quoted value");
    assert_eq!(
        request,
        Request::Set {
            key: "greeting".into(),
            value: "hello world".into(),
        }
    );
    assert_eq!(
        code(parse_command("get key extra").expect_err("extra get argument")),
        ErrorCode::ExtraArgument
    );
    assert_eq!(
        code(parse_command("set key").expect_err("missing set value")),
        ErrorCode::MissingArgument
    );
}

#[test]
fn malformed_cli_commands_fail_loudly() {
    assert_eq!(
        code(parse_command("").expect_err("missing command")),
        ErrorCode::MissingArgument
    );
    assert_eq!(
        code(parse_command("unknown key").expect_err("unknown command")),
        ErrorCode::UnknownCommand
    );
    for command in ["keys extra", "ping extra", "quit extra"] {
        assert_eq!(
            code(parse_command(command).expect_err("extra CLI argument")),
            ErrorCode::ExtraArgument,
            "command should be rejected: {command:?}"
        );
    }
    assert_eq!(
        code(parse_command(r#"set key "unterminated"#).expect_err("bad quote")),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn request_and_response_json_lines_round_trip() {
    let request = Request::Set {
        key: "中文".into(),
        value: "value with spaces".into(),
    };
    let request_line = encode_request_line(&request).expect("encode request");
    assert_eq!(request_line.last(), Some(&b'\n'));
    assert_eq!(
        parse_request_bytes(&request_line).expect("decode request"),
        request
    );

    let response = Response::success(ResponseData::Keys {
        keys: vec!["a".into(), "中文".into()],
        count: 2,
    });
    let response_line = encode_response_line(&response).expect("encode response");
    assert_eq!(
        parse_response_line(std::str::from_utf8(&response_line).expect("UTF-8 response"))
            .expect("decode response"),
        response
    );
}

#[test]
fn response_json_uses_the_frozen_nested_success_and_error_shapes() {
    let success = Response::success(ResponseData::Set { replaced: false });
    let success_line = encode_response_line(&success).expect("encode success response");
    let success_json = serde_json::to_string(&success).expect("serialize success response");
    assert_eq!(
        success_json,
        r#"{"ok":true,"data":{"kind":"set","replaced":false}}"#
    );
    assert_eq!(
        std::str::from_utf8(&success_line).expect("success response is UTF-8"),
        "{\"ok\":true,\"data\":{\"kind\":\"set\",\"replaced\":false}}\n"
    );
    assert_eq!(
        success_line,
        format!(
            "{}\n",
            serde_json::to_string(&success).expect("serialize success response")
        )
        .into_bytes()
    );
    let success_json: serde_json::Value =
        serde_json::from_slice(&success_line).expect("success response is JSON");
    assert_eq!(
        success_json,
        serde_json::json!({
            "ok": true,
            "data": {"kind": "set", "replaced": false}
        })
    );
    assert_eq!(
        parse_response_line(std::str::from_utf8(&success_line).expect("UTF-8 response"))
            .expect("decode success response"),
        success
    );

    let error = Response::error(ErrorCode::NotFound, "missing key");
    let error_line = encode_response_line(&error).expect("encode error response");
    let error_json = serde_json::to_string(&error).expect("serialize error response");
    assert_eq!(
        error_json,
        r#"{"ok":false,"error":{"code":"NOT_FOUND","message":"missing key"}}"#
    );
    assert_eq!(
        std::str::from_utf8(&error_line).expect("error response is UTF-8"),
        "{\"ok\":false,\"error\":{\"code\":\"NOT_FOUND\",\"message\":\"missing key\"}}\n"
    );
    assert_eq!(
        error_line,
        format!(
            "{}\n",
            serde_json::to_string(&error).expect("serialize error response")
        )
        .into_bytes()
    );
    let error_json: serde_json::Value =
        serde_json::from_slice(&error_line).expect("error response is JSON");
    assert_eq!(
        error_json,
        serde_json::json!({
            "ok": false,
            "error": {"code": "NOT_FOUND", "message": "missing key"}
        })
    );
    assert_eq!(
        parse_response_line(std::str::from_utf8(&error_line).expect("UTF-8 response"))
            .expect("decode error response"),
        error
    );
}

#[test]
fn json_lines_require_a_terminated_nonempty_frame() {
    for frame in ["\n", "\r\n"] {
        assert_eq!(
            code(parse_request_line(frame).expect_err("empty request frame")),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            code(parse_response_line(frame).expect_err("empty response frame")),
            ErrorCode::InvalidRequest
        );
    }

    let request = r#"{"cmd":"ping"}"#;
    assert_eq!(
        code(parse_request_line(request).expect_err("request without LF")),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(parse_request_line(&format!("{request}\r")).expect_err("request without LF")),
        ErrorCode::InvalidRequest
    );
    let response = r#"{"ok":true,"data":{"kind":"ping"}}"#;
    assert_eq!(
        code(parse_response_line(response).expect_err("response without LF")),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn json_schema_rejects_unknown_missing_extra_and_wrong_type_fields() {
    assert_eq!(
        code(
            parse_request_line(concat!(r#"{"cmd":"ping","unexpected":true}"#, "\n"))
                .expect_err("extra JSON field")
        ),
        ErrorCode::ExtraArgument
    );
    assert_eq!(
        code(
            parse_request_line(concat!(r#"{"cmd":"get"}"#, "\n")).expect_err("missing JSON field")
        ),
        ErrorCode::MissingArgument
    );
    assert_eq!(
        code(
            parse_request_line(concat!(r#"{"cmd":"get","key":"k","extra":1}"#, "\n"))
                .expect_err("extra JSON field")
        ),
        ErrorCode::ExtraArgument
    );
    assert_eq!(
        code(
            parse_request_line(concat!(r#"{"cmd":"set","key":"k","value":7}"#, "\n"))
                .expect_err("wrong value type")
        ),
        ErrorCode::InvalidValue
    );
    assert_eq!(
        code(
            parse_request_line(concat!(r#"{"cmd":"set","key":7,"value":"v"}"#, "\n"))
                .expect_err("wrong key type")
        ),
        ErrorCode::InvalidKey
    );
    assert_eq!(
        code(parse_request_line(concat!(r#"{"cmd":7}"#, "\n")).expect_err("wrong command type")),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(
            parse_request_line(concat!(r#"{"cmd":"does_not_exist"}"#, "\n"))
                .expect_err("unknown JSON command")
        ),
        ErrorCode::UnknownCommand
    );
    assert_eq!(
        code(parse_request_line("not JSON\n").expect_err("JSON syntax error")),
        ErrorCode::InvalidJson
    );
    assert_eq!(
        code(parse_request_line("[]\n").expect_err("non-object JSON request")),
        ErrorCode::InvalidRequest
    );
}

#[test]
fn response_json_shape_is_strict_at_the_public_decode_entries() {
    let invalid = [
        r#"{"ok":true}"#,
        r#"{"ok":true,"data":{"kind":"ping"},"error":{"code":"NOT_FOUND","message":"x"}}"#,
        r#"{"ok":true,"error":{"code":"NOT_FOUND","message":"x"}}"#,
        r#"{"ok":false,"data":{"kind":"ping"}}"#,
        r#"{"ok":"true","data":{"kind":"ping"}}"#,
        r#"{"ok":true,"extra":1,"data":{"kind":"ping"}}"#,
        r#"{"ok":true,"data":[]}"#,
        r#"{"ok":true,"data":{"kind":"ping","extra":1}}"#,
        r#"{"ok":true,"data":{"kind":"unknown"}}"#,
        r#"{"ok":true,"data":{"kind":"set","replaced":"false"}}"#,
        r#"{"ok":true,"data":{"kind":"keys","keys":["a"],"count":2}}"#,
        r#"{"ok":false,"error":{"code":"NOT_A_CODE","message":"x"}}"#,
        r#"{"ok":false,"error":{"code":"NOT_FOUND","message":7}}"#,
        r#"{"ok":false,"error":{"code":"NOT_FOUND","message":"x","extra":1}}"#,
    ];
    for json in invalid {
        assert_eq!(
            code(parse_response_line(&format!("{json}\n")).expect_err("invalid response shape")),
            ErrorCode::InvalidRequest,
            "response should be rejected: {json}"
        );
    }

    let valid = [
        r#"{"ok":true,"data":{"kind":"set","replaced":false}}"#,
        r#"{"ok":true,"data":{"kind":"get","value":"v"}}"#,
        r#"{"ok":true,"data":{"kind":"delete","deleted":true}}"#,
        r#"{"ok":true,"data":{"kind":"keys","keys":["a","b"],"count":2}}"#,
        r#"{"ok":true,"data":{"kind":"status","count":2}}"#,
        r#"{"ok":true,"data":{"kind":"ping"}}"#,
        r#"{"ok":true,"data":{"kind":"quit"}}"#,
        r#"{"ok":false,"error":{"code":"STORAGE_ERROR","message":"failure"}}"#,
    ];
    for json in valid {
        parse_response_line(&format!("{json}\n")).expect("valid response shape");
    }

    assert_eq!(
        code(parse_response_bytes(b"{\xff}\n").expect_err("invalid response UTF-8")),
        ErrorCode::InvalidUtf8
    );
    parse_response_bytes(b"{\"ok\":true,\"data\":{\"kind\":\"ping\"}}\n")
        .expect("valid response bytes");
}

#[test]
fn protocol_limits_and_utf8_errors_have_stable_codes() {
    let oversized = format!(
        r#"{{"cmd":"set","key":"k","value":"{}"}}"#,
        "x".repeat(MAX_FRAME_BYTES)
    );
    assert_eq!(
        code(parse_request_line(&format!("{oversized}\n")).expect_err("oversized request")),
        ErrorCode::FrameTooLarge
    );
    assert_eq!(
        code(parse_request_bytes(b"{\xff}\n").expect_err("invalid UTF-8")),
        ErrorCode::InvalidUtf8
    );
}

#[test]
fn key_and_value_boundaries_are_measured_in_utf8_bytes() {
    let mut store = Store::new();
    let key = "k".repeat(MAX_KEY_BYTES);
    let value = "v".repeat(MAX_VALUE_BYTES);
    assert!(
        !store
            .set(&key, &value)
            .expect("maximum fields are valid")
            .replaced
    );

    assert_eq!(
        code(
            store
                .set(&"k".repeat(MAX_KEY_BYTES + 1), "v")
                .expect_err("key over limit")
        ),
        ErrorCode::InvalidKey
    );
    assert_eq!(
        code(
            store
                .set("too-large-value", &"v".repeat(MAX_VALUE_BYTES + 1))
                .expect_err("value over limit")
        ),
        ErrorCode::InvalidValue
    );
    assert_eq!(
        code(store.set("", "v").expect_err("empty key")),
        ErrorCode::InvalidKey
    );
    assert_eq!(
        code(store.set("bad key", "v").expect_err("whitespace key")),
        ErrorCode::InvalidKey
    );
    assert_eq!(
        code(store.set("bad\nkey", "v").expect_err("control key")),
        ErrorCode::InvalidKey
    );
    assert_eq!(
        code(store.set("empty-value", "").expect_err("empty value")),
        ErrorCode::InvalidValue
    );
    assert_eq!(
        code(
            store
                .set("control-value", "bad\nvalue")
                .expect_err("control value")
        ),
        ErrorCode::InvalidValue
    );
}

#[test]
fn chinese_and_space_values_are_valid_business_data() {
    let mut store = Store::new();
    store.set("问候", "你好，Rust 世界").expect("Unicode data");
    assert_eq!(
        store.get("问候").expect("stored Unicode data"),
        "你好，Rust 世界"
    );
}

#[test]
fn crud_overwrite_sorting_and_count_match_store_contract() {
    let mut store = Store::new();
    assert_eq!(store.len(), 0);
    assert!(store.is_empty());
    assert!(!store.set("b", "two").expect("first insert").replaced);
    assert!(!store.set("a", "one").expect("second insert").replaced);
    assert!(store.set("b", "updated").expect("overwrite").replaced);
    assert_eq!(store.get("b").expect("overwritten value"), "updated");
    assert_eq!(store.keys(), vec!["a".to_owned(), "b".to_owned()]);
    assert_eq!(store.len(), 2);
    assert_eq!(store.status().count, 2);
    assert!(store.delete("a").expect("existing delete").deleted);
    assert_eq!(store.keys(), vec!["b".to_owned()]);
    assert_eq!(
        code(store.get("missing").expect_err("missing get")),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(store.delete("missing").expect_err("missing delete")),
        ErrorCode::NotFound
    );
}
