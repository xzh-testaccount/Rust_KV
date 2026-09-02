use std::{
    io::{BufReader, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Barrier,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use rust_kv_store::{
    error::ErrorCode,
    persistence::PersistentStore,
    protocol::{
        Frame, MAX_FRAME_BYTES, Request, Response, ResponseData, encode_request_line,
        parse_response_bytes, read_frame,
    },
    server::{self, LockStrategy, RuntimeMode, SharedStore},
};
use tempfile::TempDir;

const CLIENTS: usize = 6;
const WRITES_PER_CLIENT: usize = 8;

fn combinations() -> [(RuntimeMode, LockStrategy); 4] {
    [
        (RuntimeMode::Sync, LockStrategy::Mutex),
        (RuntimeMode::Sync, LockStrategy::RwLock),
        (RuntimeMode::Async, LockStrategy::Mutex),
        (RuntimeMode::Async, LockStrategy::RwLock),
    ]
}

struct TestServer {
    address: SocketAddr,
    wal_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    task: Option<thread::JoinHandle<rust_kv_store::error::Result<()>>>,
}

impl TestServer {
    fn start(temp: &TempDir, runtime: RuntimeMode, lock: LockStrategy, name: &str) -> Self {
        Self::start_path(temp.path().join(format!("{name}.wal")), runtime, lock)
    }

    fn start_path(wal_path: PathBuf, runtime: RuntimeMode, lock: LockStrategy) -> Self {
        let persistent = PersistentStore::open(&wal_path).expect("test store opens");
        let store = SharedStore::new(persistent, lock);
        let listener = TcpListener::bind("127.0.0.1:0").expect("test listener binds");
        let address = listener.local_addr().expect("listener has an address");
        let shutdown = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&shutdown);

        let task = thread::spawn(move || match runtime {
            RuntimeMode::Sync => server::serve_sync_until(listener, store, signal),
            RuntimeMode::Async => {
                listener.set_nonblocking(true)?;
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()?;
                runtime.block_on(async move {
                    let listener = tokio::net::TcpListener::from_std(listener)?;
                    server::serve_async_until(listener, store, async move {
                        while !signal.load(Ordering::Acquire) {
                            tokio::time::sleep(Duration::from_millis(2)).await;
                        }
                    })
                    .await
                })
            }
        });

        Self {
            address,
            wal_path,
            shutdown,
            task: Some(task),
        }
    }

    fn stop(mut self) -> PathBuf {
        self.shutdown.store(true, Ordering::Release);
        self.task
            .take()
            .expect("server task exists")
            .join()
            .expect("server thread does not panic")
            .expect("server exits cleanly");
        self.wal_path.clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(task) = self.task.take() {
            let _ = task.join();
        }
    }
}

struct WireClient {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl WireClient {
    fn connect(address: SocketAddr) -> Self {
        let writer = TcpStream::connect(address).expect("client connects");
        writer
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        writer
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let reader = BufReader::new(writer.try_clone().expect("stream clones"));
        Self { reader, writer }
    }

    fn request(&mut self, request: Request) -> Response {
        let encoded = encode_request_line(&request).expect("request encodes");
        self.send_raw(&encoded);
        self.response()
    }

    fn send_raw(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).expect("request writes");
        self.writer.flush().expect("request flushes");
    }

    fn response(&mut self) -> Response {
        let frame = read_frame(&mut self.reader).expect("response frame reads");
        let Frame::Line(line) = frame else {
            panic!("expected response line, got {frame:?}");
        };
        parse_response_bytes(&line).expect("response parses")
    }
}

fn error_code(response: Response) -> ErrorCode {
    assert!(!response.ok);
    response.error.expect("failure has an error").code
}

fn label(runtime: RuntimeMode, lock: LockStrategy) -> String {
    format!("{runtime}-{lock}")
}

fn assert_recovered(path: &Path, expected_entries: usize, expected_records: u64) {
    let recovered = PersistentStore::open(path).expect("WAL recovers");
    assert_eq!(recovered.len(), expected_entries);
    assert_eq!(recovered.stats().wal_records, expected_records);
}

#[test]
fn all_four_combinations_complete_crud_and_restart_from_the_same_wal() {
    let temp = tempfile::tempdir().expect("temp directory is created");

    for (runtime, lock) in combinations() {
        let server = TestServer::start(&temp, runtime, lock, &label(runtime, lock));
        let mut client = WireClient::connect(server.address);

        assert_eq!(
            client
                .request(Request::Set {
                    key: "course".to_owned(),
                    value: "Rust".to_owned(),
                })
                .data,
            Some(ResponseData::Set { replaced: false })
        );
        assert_eq!(
            client
                .request(Request::Set {
                    key: "course".to_owned(),
                    value: "Advanced Rust".to_owned(),
                })
                .data,
            Some(ResponseData::Set { replaced: true })
        );
        assert_eq!(
            client
                .request(Request::Set {
                    key: "temporary".to_owned(),
                    value: "value".to_owned(),
                })
                .data,
            Some(ResponseData::Set { replaced: false })
        );
        assert_eq!(
            client
                .request(Request::Delete {
                    key: "temporary".to_owned(),
                })
                .data,
            Some(ResponseData::Delete { deleted: true })
        );
        assert_eq!(
            error_code(client.request(Request::Get {
                key: "temporary".to_owned(),
            })),
            ErrorCode::NotFound
        );
        assert_eq!(client.request(Request::Ping).data, Some(ResponseData::Ping));
        assert_eq!(client.request(Request::Quit).data, Some(ResponseData::Quit));
        drop(client);

        let wal_path = server.stop();
        let restarted = TestServer::start_path(wal_path.clone(), runtime, lock);
        let mut client = WireClient::connect(restarted.address);
        assert_eq!(
            client
                .request(Request::Get {
                    key: "course".to_owned(),
                })
                .data,
            Some(ResponseData::Get {
                value: "Advanced Rust".to_owned(),
            })
        );
        assert_eq!(
            client.request(Request::Keys).data,
            Some(ResponseData::Keys {
                keys: vec!["course".to_owned()],
                count: 1,
            })
        );
        client.request(Request::Quit);
        drop(client);
        restarted.stop();

        assert_recovered(&wal_path, 1, 4);
    }
}

#[test]
fn all_four_combinations_keep_concurrent_writes_ordered_and_recoverable() {
    let temp = tempfile::tempdir().expect("temp directory is created");

    for (runtime, lock) in combinations() {
        let server = TestServer::start(
            &temp,
            runtime,
            lock,
            &format!("concurrent-{}", label(runtime, lock)),
        );
        let barrier = Arc::new(Barrier::new(CLIENTS));
        let mut clients = Vec::new();

        for client_id in 0..CLIENTS {
            let address = server.address;
            let barrier = Arc::clone(&barrier);
            clients.push(thread::spawn(move || {
                let mut client = WireClient::connect(address);
                barrier.wait();
                for operation in 0..WRITES_PER_CLIENT {
                    let key = format!("client{client_id:02}-key{operation:02}");
                    let value = format!("value-{client_id}-{operation}");
                    assert!(
                        client
                            .request(Request::Set {
                                key: key.clone(),
                                value: value.clone(),
                            })
                            .ok
                    );
                    assert_eq!(
                        client.request(Request::Get { key }).data,
                        Some(ResponseData::Get { value })
                    );
                }
                client.request(Request::Quit);
            }));
        }

        for client in clients {
            client.join().expect("client thread does not panic");
        }

        let mut verifier = WireClient::connect(server.address);
        assert_eq!(
            verifier.request(Request::Status).data,
            Some(ResponseData::Status {
                count: CLIENTS * WRITES_PER_CLIENT,
            })
        );
        verifier.request(Request::Quit);
        drop(verifier);

        let wal_path = server.stop();
        assert_recovered(
            &wal_path,
            CLIENTS * WRITES_PER_CLIENT,
            (CLIENTS * WRITES_PER_CLIENT) as u64,
        );
    }
}

#[test]
fn all_four_combinations_report_storage_and_compact_without_losing_data() {
    let temp = tempfile::tempdir().expect("temp directory is created");

    for (runtime, lock) in combinations() {
        let server = TestServer::start(
            &temp,
            runtime,
            lock,
            &format!("compact-{}", label(runtime, lock)),
        );
        let mut client = WireClient::connect(server.address);

        for (key, value) in [("zeta", "one"), ("alpha", "first"), ("zeta", "two")] {
            assert!(
                client
                    .request(Request::Set {
                        key: key.to_owned(),
                        value: value.to_owned(),
                    })
                    .ok
            );
        }

        let status = client.request(Request::StorageStatus);
        let Some(ResponseData::StorageStatus {
            entries,
            wal_records,
            wal_bytes,
            snapshot_bytes,
            last_sequence,
            writable,
        }) = status.data
        else {
            panic!("expected storage status, got {status:?}");
        };
        assert_eq!(entries, 2);
        assert_eq!(wal_records, 3);
        assert!(wal_bytes > 0);
        assert_eq!(snapshot_bytes, 0);
        assert_eq!(last_sequence, 3);
        assert!(writable);

        let compacted = client.request(Request::Compact);
        let Some(ResponseData::Compact {
            entries,
            wal_records_before,
            wal_bytes_before,
            snapshot_bytes_before,
            last_sequence_before,
            wal_records_after,
            wal_bytes_after,
            snapshot_bytes_after,
            last_sequence_after,
            ..
        }) = compacted.data
        else {
            panic!("expected compaction result, got {compacted:?}");
        };
        assert_eq!(entries, 2);
        assert_eq!(wal_records_before, 3);
        assert!(wal_bytes_before > 0);
        assert_eq!(snapshot_bytes_before, 0);
        assert_eq!(last_sequence_before, 3);
        assert_eq!(wal_records_after, 0);
        assert_eq!(wal_bytes_after, 0);
        assert!(snapshot_bytes_after > 0);
        assert_eq!(last_sequence_after, 3);

        let status = client.request(Request::StorageStatus);
        assert!(matches!(
            status.data,
            Some(ResponseData::StorageStatus {
                entries: 2,
                wal_records: 0,
                wal_bytes: 0,
                snapshot_bytes,
                last_sequence: 3,
                writable: true,
            }) if snapshot_bytes > 0
        ));
        client.request(Request::Quit);
        drop(client);

        let wal_path = server.stop();
        assert!(wal_path.with_extension("snapshot").is_file());
        let restarted = TestServer::start_path(wal_path.clone(), runtime, lock);
        let mut client = WireClient::connect(restarted.address);
        assert_eq!(
            client
                .request(Request::Get {
                    key: "zeta".to_owned(),
                })
                .data,
            Some(ResponseData::Get {
                value: "two".to_owned(),
            })
        );
        assert!(
            client
                .request(Request::Set {
                    key: "after-compact".to_owned(),
                    value: "recoverable".to_owned(),
                })
                .ok
        );
        assert!(matches!(
            client.request(Request::StorageStatus).data,
            Some(ResponseData::StorageStatus {
                entries: 3,
                wal_records: 1,
                wal_bytes,
                snapshot_bytes,
                last_sequence: 4,
                writable: true,
            }) if wal_bytes > 0 && snapshot_bytes > 0
        ));
        client.request(Request::Quit);
        drop(client);
        restarted.stop();

        let recovered = PersistentStore::open(&wal_path).expect("snapshot and WAL recover");
        assert_eq!(recovered.len(), 3);
        assert_eq!(recovered.get("alpha").unwrap(), "first");
        assert_eq!(recovered.get("zeta").unwrap(), "two");
        assert_eq!(recovered.get("after-compact").unwrap(), "recoverable");
        assert_eq!(recovered.last_sequence(), 4);
    }
}

#[test]
fn all_four_combinations_handle_partial_sticky_invalid_and_oversized_frames() {
    let temp = tempfile::tempdir().expect("temp directory is created");

    for (runtime, lock) in combinations() {
        let server = TestServer::start(
            &temp,
            runtime,
            lock,
            &format!("frames-{}", label(runtime, lock)),
        );
        let mut client = WireClient::connect(server.address);

        client.send_raw(br#"{"cmd":"pi"#);
        thread::sleep(Duration::from_millis(10));
        client.send_raw(b"ng\"}\n");
        assert_eq!(client.response().data, Some(ResponseData::Ping));

        client.send_raw(b"{\"cmd\":\"ping\"}\n{\"cmd\":\"status\"}\n");
        assert_eq!(client.response().data, Some(ResponseData::Ping));
        assert_eq!(
            client.response().data,
            Some(ResponseData::Status { count: 0 })
        );

        client.send_raw(b"{not-json}\n{\"cmd\":\"ping\"}\n");
        assert_eq!(error_code(client.response()), ErrorCode::InvalidJson);
        assert_eq!(client.response().data, Some(ResponseData::Ping));

        client.send_raw(b"\xFF\n{\"cmd\":\"ping\"}\n");
        assert_eq!(error_code(client.response()), ErrorCode::InvalidUtf8);
        assert_eq!(client.response().data, Some(ResponseData::Ping));

        let mut oversized = vec![b'x'; MAX_FRAME_BYTES + 1];
        oversized.extend_from_slice(b"\n{\"cmd\":\"ping\"}\n");
        client.send_raw(&oversized);
        assert_eq!(error_code(client.response()), ErrorCode::FrameTooLarge);
        assert_eq!(client.response().data, Some(ResponseData::Ping));

        assert_eq!(client.request(Request::Quit).data, Some(ResponseData::Quit));
        drop(client);

        let mut abandoned = TcpStream::connect(server.address).expect("client connects");
        abandoned.write_all(b"{\"cmd\":").unwrap();
        drop(abandoned);
        thread::sleep(Duration::from_millis(10));

        let mut survivor = WireClient::connect(server.address);
        assert_eq!(
            survivor.request(Request::Ping).data,
            Some(ResponseData::Ping)
        );
        survivor.request(Request::Quit);
        drop(survivor);
        server.stop();
    }
}
