use rust_kv_store::client::{ClientConfig, parse_client_args, run_with_io};
use rust_kv_store::error::{AppError, ErrorCode};
use rust_kv_store::protocol::{Response, ResponseData, encode_response_line};
use std::collections::VecDeque;
use std::io::Cursor;

fn response(data: ResponseData) -> Vec<u8> {
    encode_response_line(&Response::success(data)).expect("response encoding")
}

fn error_response(code: ErrorCode, message: &str) -> Vec<u8> {
    encode_response_line(&Response::error(code, message)).expect("error encoding")
}

#[test]
fn defaults_help_and_strict_options_are_stable() {
    assert_eq!(
        parse_client_args(Vec::<String>::new()).expect("defaults"),
        Some(ClientConfig {
            server: "127.0.0.1:7878".to_owned(),
        })
    );
    assert_eq!(
        parse_client_args(["--server", "localhost:9000"]).expect("server option"),
        Some(ClientConfig {
            server: "localhost:9000".to_owned(),
        })
    );
    assert!(parse_client_args(["--help"]).expect("help").is_none());
    assert_eq!(
        ClientConfig::parse(["kv-client", "--server", "localhost:9000"])
            .expect("program and server option"),
        Some(ClientConfig {
            server: "localhost:9000".to_owned(),
        })
    );
    for (args, expected_code) in [
        (vec!["--unknown"], ErrorCode::UnknownCommand),
        (vec!["--help", "--unknown"], ErrorCode::UnknownCommand),
        (vec!["--unknown", "--help"], ErrorCode::UnknownCommand),
        (vec!["--server"], ErrorCode::MissingArgument),
        (
            vec!["--server", "a", "--server", "b"],
            ErrorCode::ExtraArgument,
        ),
        (vec!["--server", "a", "--help"], ErrorCode::ExtraArgument),
        (vec!["--help", "--server", "a"], ErrorCode::ExtraArgument),
        (vec!["--help", "--help"], ErrorCode::ExtraArgument),
        (vec!["positional"], ErrorCode::ExtraArgument),
    ] {
        let error = parse_client_args(args).expect_err("invalid client options");
        assert_eq!(error.code(), expected_code);
    }
}

#[test]
fn local_command_errors_do_not_send_network_requests() {
    let mut sent = Vec::new();
    let mut replies = VecDeque::from([response(ResponseData::Ping), response(ResponseData::Quit)]);
    let mut output = Vec::new();
    run_with_io(
        Cursor::new("get\nping\nquit\n"),
        &mut output,
        |request| {
            sent.push(request.to_vec());
            Ok(())
        },
        || Ok(replies.pop_front().expect("reply")),
    )
    .expect("client loop");
    assert_eq!(sent.len(), 2);
    assert!(
        String::from_utf8(output)
            .expect("output")
            .contains("[MISSING_ARGUMENT]")
    );
}

#[test]
fn success_responses_are_friendly_and_quit_says_bye() {
    let mut replies = VecDeque::from([
        response(ResponseData::Set { replaced: false }),
        response(ResponseData::Set { replaced: true }),
        response(ResponseData::Get {
            value: "Alice".to_owned(),
        }),
        response(ResponseData::Delete { deleted: true }),
        response(ResponseData::Keys {
            keys: vec!["a".to_owned(), "b".to_owned()],
            count: 2,
        }),
        response(ResponseData::Status { count: 2 }),
        response(ResponseData::Ping),
        response(ResponseData::Quit),
    ]);
    let mut output = Vec::new();
    run_with_io(
        Cursor::new(
            "set name Alice\nset name Bob\nget name\ndelete name\nkeys\nstatus\nping\nquit\n",
        ),
        &mut output,
        |_| Ok(()),
        || Ok(replies.pop_front().expect("reply")),
    )
    .expect("client loop");
    let output = String::from_utf8(output).expect("output");
    for expected in [
        "created",
        "replaced",
        "value: Alice",
        "deleted: true",
        "keys (2): a, b",
        "status: count=2",
        "pong",
        "bye",
    ] {
        assert!(
            output.contains(expected),
            "missing {expected:?} in {output:?}"
        );
    }
}

#[test]
fn quoted_values_and_server_errors_continue_until_quit() {
    let mut replies = VecDeque::from([
        error_response(ErrorCode::NotFound, "missing key"),
        response(ResponseData::Get {
            value: "hello world".to_owned(),
        }),
        response(ResponseData::Quit),
    ]);
    let mut output = Vec::new();
    run_with_io(
        Cursor::new("get missing\nset greeting \"hello world\"\nquit\n"),
        &mut output,
        |_| Ok(()),
        || Ok(replies.pop_front().expect("reply")),
    )
    .expect("client loop");
    let output = String::from_utf8(output).expect("output");
    assert!(output.contains("[NOT_FOUND] missing key"));
    assert!(output.contains("value: hello world"));
}

#[test]
fn quit_only_exits_after_receiving_quit_response() {
    let mut replies = VecDeque::from([
        error_response(ErrorCode::NotFound, "not yet"),
        response(ResponseData::Quit),
    ]);
    let mut output = Vec::new();
    run_with_io(
        Cursor::new("quit\nquit\n"),
        &mut output,
        |_| Ok(()),
        || Ok(replies.pop_front().expect("reply")),
    )
    .expect("client loop");
    let output = String::from_utf8(output).expect("output");
    assert!(output.contains("[NOT_FOUND] not yet"));
    assert!(output.contains("bye"));
}

#[test]
fn disconnect_and_bad_response_are_reported_and_stop_client() {
    let mut output = Vec::new();
    run_with_io(
        Cursor::new("ping\n"),
        &mut output,
        |_| Ok(()),
        || Err(AppError::Message("disconnected".to_owned())),
    )
    .expect_err("disconnect must stop client");
    assert!(
        String::from_utf8(output)
            .expect("output")
            .contains("disconnected")
    );

    let mut output = Vec::new();
    run_with_io(
        Cursor::new("ping\n"),
        &mut output,
        |_| Ok(()),
        || Ok(b"not-json\n".to_vec()),
    )
    .expect_err("bad response must stop client");
    assert!(
        String::from_utf8(output)
            .expect("output")
            .contains("[INVALID_JSON]")
    );
}
