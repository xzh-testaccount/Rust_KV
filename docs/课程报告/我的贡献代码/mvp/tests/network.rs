//! TCP integration tests for framing, dispatch, isolation, and concurrency.

use rust_kv_store::error::ErrorCode;
use rust_kv_store::persistence::PersistentStore;
use rust_kv_store::protocol::{
    Frame, MAX_FRAME_BYTES, Request, Response, ResponseData, encode_request_line,
    parse_response_line, read_frame,
};
use rust_kv_store::server::{self, ServerConfig};
use std::io::{BufReader, Cursor, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tempfile::{TempDir, tempdir};

struct RunningServer {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    join: thread::JoinHandle<rust_kv_store::error::Result<()>>,
    store: Arc<Mutex<PersistentStore>>,
    directory: TempDir,
    data_path: std::path::PathBuf,
}

impl RunningServer {
    fn start() -> Self {
        let directory = tempdir().expect("temporary directory");
        let data_path = directory.path().join("nested").join("kv.wal");
        let config = ServerConfig {
            bind: "127.0.0.1:0".parse().expect("loopback address"),
            data: data_path.clone(),
        };
        let listener = server::bind(&config).expect("bind test listener");
        let address = listener.local_addr().expect("listener address");
        let store = Arc::new(Mutex::new(server::open(&config).expect("open test WAL")));
        let stop = Arc::new(AtomicBool::new(false));
        let join = {
            let stop = Arc::clone(&stop);
            let shared_store = Arc::clone(&store);
            thread::spawn(move || server::serve(listener, shared_store, stop))
        };
        Self {
            address,
            stop,
            join,
            store,
            directory,
            data_path,
        }
    }

    fn stop(self) -> (TempDir, std::path::PathBuf) {
        self.stop.store(true, Ordering::Release);
        let result = self.join.join().expect("server thread");
        result.expect("server stopped cleanly");
        (self.directory, self.data_path)
    }
}

struct WireClient {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
}

impl WireClient {
    fn connect(address: SocketAddr) -> Self {
        let writer = TcpStream::connect_timeout(&address, Duration::from_secs(2))
            .expect("connect test server");
        writer
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set read timeout");
        let reader_stream = writer.try_clone().expect("clone test stream");
        Self {
            writer,
            reader: BufReader::new(reader_stream),
        }
    }

    fn request(&mut self, request: &Request) -> Response {
        let encoded = encode_request_line(request).expect("encode request");
        self.writer.write_all(&encoded).expect("write request");
        self.writer.flush().expect("flush request");
        read_response(&mut self.reader)
    }
}

fn read_response(reader: &mut BufReader<TcpStream>) -> Response {
    match read_frame(reader).expect("read response frame") {
        Frame::Line(line) => {
            parse_response_line(std::str::from_utf8(&line).expect("UTF-8 response"))
                .expect("parse response")
        }
        frame => panic!("unexpected response frame: {frame:?}"),
    }
}

fn error_code(response: &Response) -> ErrorCode {
    response.error.as_ref().expect("error response").code
}

fn config_error_code(args: &[&str]) -> ErrorCode {
    ServerConfig::parse(args.iter().copied())
        .expect_err("invalid server arguments")
        .code()
}

#[test]
fn frame_reader_handles_split_sticky_and_crlf_frames() {
    let bytes = b"one\r\ntwo\n";
    let mut reader = BufReader::new(Cursor::new(bytes));
    assert_eq!(
        read_frame(&mut reader).expect("first frame"),
        Frame::Line(b"one\r\n".to_vec())
    );
    assert_eq!(
        read_frame(&mut reader).expect("second frame"),
        Frame::Line(b"two\n".to_vec())
    );
}

#[test]
fn tcp_crlf_and_empty_frames_continue_to_a_later_ping() {
    let server = RunningServer::start();
    let mut client = WireClient::connect(server.address);
    client
        .writer
        .write_all(b"{\"cmd\":\"ping\"}\r\n\n{\"cmd\":\"ping\"}\n")
        .expect("write CRLF and empty frames");
    client.writer.flush().expect("flush frames");
    assert!(matches!(
        read_response(&mut client.reader).data,
        Some(ResponseData::Ping)
    ));
    assert_eq!(
        error_code(&read_response(&mut client.reader)),
        ErrorCode::InvalidRequest
    );
    assert!(matches!(
        read_response(&mut client.reader).data,
        Some(ResponseData::Ping)
    ));
    let _ = server.stop();
}

#[test]
fn split_and_sticky_requests_receive_ordered_responses() {
    let server = RunningServer::start();
    let mut client = WireClient::connect(server.address);
    let set = encode_request_line(&Request::Set {
        key: "split".into(),
        value: "value".into(),
    })
    .expect("encode set");
    let split = set.len() / 2;
    client
        .writer
        .write_all(&set[..split])
        .expect("write first request fragment");
    thread::sleep(Duration::from_millis(10));
    let get = encode_request_line(&Request::Get {
        key: "split".into(),
    })
    .expect("encode get");
    client
        .writer
        .write_all(&[set[split..].to_vec(), get].concat())
        .expect("write remaining and sticky request");
    client.writer.flush().expect("flush requests");
    assert!(matches!(
        read_response(&mut client.reader).data,
        Some(ResponseData::Set { replaced: false })
    ));
    assert!(matches!(
        read_response(&mut client.reader).data,
        Some(ResponseData::Get { value }) if value == "value"
    ));
    let _ = server.stop();
}

#[test]
fn invalid_utf8_and_json_are_errors_then_ping_continues() {
    let server = RunningServer::start();
    let mut client = WireClient::connect(server.address);
    client
        .writer
        .write_all(b"{\xff}\nnot-json\n{\"cmd\":\"ping\"}\n")
        .expect("write invalid requests");
    client.writer.flush().expect("flush invalid requests");
    assert_eq!(
        error_code(&read_response(&mut client.reader)),
        ErrorCode::InvalidUtf8
    );
    assert_eq!(
        error_code(&read_response(&mut client.reader)),
        ErrorCode::InvalidJson
    );
    assert!(matches!(
        read_response(&mut client.reader).data,
        Some(ResponseData::Ping)
    ));
    let _ = server.stop();
}

#[test]
fn oversized_request_is_discarded_then_ping_continues() {
    let server = RunningServer::start();
    let mut client = WireClient::connect(server.address);
    let mut oversized = vec![b'x'; MAX_FRAME_BYTES + 1];
    oversized.push(b'\n');
    oversized.extend_from_slice(b"{\"cmd\":\"ping\"}\n");
    client
        .writer
        .write_all(&oversized)
        .expect("write oversized request");
    client.writer.flush().expect("flush oversized request");
    assert_eq!(
        error_code(&read_response(&mut client.reader)),
        ErrorCode::FrameTooLarge
    );
    assert!(matches!(
        read_response(&mut client.reader).data,
        Some(ResponseData::Ping)
    ));
    let _ = server.stop();
}

#[test]
fn tcp_payload_at_exact_frame_limit_is_accepted() {
    let server = RunningServer::start();
    let mut client = WireClient::connect(server.address);
    let mut request = br#"{"cmd":"ping"}"#.to_vec();
    request.resize(MAX_FRAME_BYTES, b' ');
    request.push(b'\n');
    client
        .writer
        .write_all(&request)
        .expect("write exact-limit request");
    client.writer.flush().expect("flush exact-limit request");
    assert!(matches!(
        read_response(&mut client.reader).data,
        Some(ResponseData::Ping)
    ));
    let _ = server.stop();
}

#[test]
fn oversized_unterminated_request_returns_error_and_closes_connection() {
    let server = RunningServer::start();
    let mut client = WireClient::connect(server.address);
    client
        .writer
        .write_all(&vec![b'x'; MAX_FRAME_BYTES + 1])
        .expect("write oversized unterminated request");
    client
        .writer
        .shutdown(Shutdown::Write)
        .expect("shutdown oversized request");
    let response = read_response(&mut client.reader);
    assert!(
        matches!(
            error_code(&response),
            ErrorCode::FrameTooLarge | ErrorCode::InvalidRequest
        ),
        "unexpected oversized unterminated response: {response:?}"
    );
    assert_eq!(
        read_frame(&mut client.reader).expect("read close after incomplete request"),
        Frame::Eof
    );
    let _ = server.stop();
}

#[test]
fn incomplete_eof_gets_invalid_request_before_connection_close() {
    let server = RunningServer::start();
    let mut client = WireClient::connect(server.address);
    client
        .writer
        .write_all(b"{\"cmd\":\"ping\"}")
        .expect("write incomplete request");
    client
        .writer
        .shutdown(Shutdown::Write)
        .expect("shutdown request side");
    assert_eq!(
        error_code(&read_response(&mut client.reader)),
        ErrorCode::InvalidRequest
    );
    let _ = server.stop();
}

#[test]
fn disconnected_client_does_not_affect_another_connection() {
    let server = RunningServer::start();
    let client = WireClient::connect(server.address);
    drop(client);
    let mut healthy = WireClient::connect(server.address);
    assert!(matches!(
        healthy.request(&Request::Ping).data,
        Some(ResponseData::Ping)
    ));
    let _ = server.stop();
}

#[test]
fn idle_client_does_not_hold_storage_lock() {
    let server = RunningServer::start();
    let idle = WireClient::connect(server.address);
    let mut active = WireClient::connect(server.address);
    assert!(matches!(
        active
            .request(&Request::Set {
                key: "active".into(),
                value: "value".into(),
            })
            .data,
        Some(ResponseData::Set { replaced: false })
    ));
    assert!(matches!(
        active
            .request(&Request::Get {
                key: "active".into()
            })
            .data,
        Some(ResponseData::Get { value }) if value == "value"
    ));
    drop(idle);
    let _ = server.stop();
}

#[test]
fn poisoned_shared_mutex_returns_storage_error_and_listener_continues() {
    let server = RunningServer::start();
    let poisoned_store = Arc::clone(&server.store);
    thread::spawn(move || {
        let _guard = poisoned_store
            .lock()
            .expect("acquire store before poisoning");
        panic!("intentionally poison shared storage mutex");
    })
    .join()
    .expect_err("poisoning thread must panic");

    let mut client = WireClient::connect(server.address);
    let response = client.request(&Request::Set {
        key: "after-poison".into(),
        value: "value".into(),
    });
    assert_eq!(error_code(&response), ErrorCode::StorageError);
    assert!(matches!(
        client.request(&Request::Ping).data,
        Some(ResponseData::Ping)
    ));
    let _ = server.stop();
}

#[test]
fn eight_clients_can_write_independent_keys_concurrently() {
    let server = RunningServer::start();
    let mut joins = Vec::new();
    for index in 0..8 {
        let address = server.address;
        joins.push(thread::spawn(move || {
            let mut client = WireClient::connect(address);
            let key = format!("client-{index}");
            assert!(matches!(
                client
                    .request(&Request::Set {
                        key: key.clone(),
                        value: format!("value-{index}"),
                    })
                    .data,
                Some(ResponseData::Set { replaced: false })
            ));
            assert!(matches!(
                client.request(&Request::Get { key }).data,
                Some(ResponseData::Get { .. })
            ));
        }));
    }
    for join in joins {
        join.join().expect("client thread");
    }
    let mut observer = WireClient::connect(server.address);
    assert!(matches!(
        observer.request(&Request::Status).data,
        Some(ResponseData::Status { count: 8 })
    ));
    let _ = server.stop();
}

#[test]
fn oversized_keys_response_is_replaced_by_small_error() {
    let server = RunningServer::start();
    let mut client = WireClient::connect(server.address);
    for index in 0..300 {
        let key = format!("key-{index:03}-{}", "x".repeat(245));
        let response = client.request(&Request::Set {
            key,
            value: "value".into(),
        });
        assert!(response.ok, "set response should succeed");
    }
    assert_eq!(
        error_code(&client.request(&Request::Keys)),
        ErrorCode::FrameTooLarge
    );
    let _ = server.stop();
}

#[test]
fn confirmed_writes_survive_server_stop_and_restart() {
    let server = RunningServer::start();
    let mut client = WireClient::connect(server.address);
    for index in 0..8 {
        assert!(
            client
                .request(&Request::Set {
                    key: format!("restart-{index}"),
                    value: format!("value-{index}"),
                })
                .ok
        );
    }
    let (_directory, data_path) = server.stop();
    let restored = PersistentStore::open(&data_path).expect("reopen confirmed WAL");
    assert_eq!(restored.len(), 8);
    for index in 0..8 {
        assert_eq!(
            restored
                .get(&format!("restart-{index}"))
                .expect("restored value"),
            format!("value-{index}")
        );
    }
}

#[test]
fn server_config_rejects_unknown_duplicate_and_missing_options() {
    assert_eq!(
        ServerConfig::parse(["kv-server"]).expect("default config"),
        Some(ServerConfig::default())
    );
    assert_eq!(
        ServerConfig::parse(["kv-server", "--help"]).expect("help"),
        None
    );
    assert_eq!(
        config_error_code(&["kv-server", "--unknown"]),
        ErrorCode::UnknownCommand
    );
    assert_eq!(
        config_error_code(&["kv-server", "--bind"]),
        ErrorCode::MissingArgument
    );
    assert_eq!(
        config_error_code(&[
            "kv-server",
            "--bind",
            "127.0.0.1:1",
            "--bind",
            "127.0.0.1:2"
        ]),
        ErrorCode::ExtraArgument
    );
    assert_eq!(
        config_error_code(&["kv-server", "--data", "one", "--data", "two"]),
        ErrorCode::ExtraArgument
    );
    assert_eq!(
        config_error_code(&["kv-server", "--help", "--bind", "127.0.0.1:1"]),
        ErrorCode::ExtraArgument
    );
    assert_eq!(
        config_error_code(&["kv-server", "--bind", "127.0.0.1:1", "--help"]),
        ErrorCode::ExtraArgument
    );
}
