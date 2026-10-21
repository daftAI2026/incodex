//! Loopback regressions for the macOS masked-window monitor.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tungstenite::Message;

#[test]
#[cfg(not(target_os = "windows"))]
fn explicit_pending_mask_observations_do_not_trigger_shutdown() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let alive = Arc::new(AtomicBool::new(true));
    let server_alive = alive.clone();
    let server = thread::spawn(move || {
        let mut observations = 0usize;
        while server_alive.load(Ordering::Acquire) {
            let Some(mut request) = accept_while_alive(&listener, &server_alive) else {
                break;
            };
            assert_eq!(read_request_path(&mut request), "/json/list");
            write_json(
                &mut request,
                &json!([{
                    "id": "main",
                    "type": "page",
                    "url": "app://-/index.html",
                    "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/main")
                }]),
            );

            let stream = accept_while_alive(&listener, &server_alive).unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            let Message::Text(command) = socket.read().unwrap() else {
                panic!("mask observation must be a CDP text command");
            };
            let command: Value = serde_json::from_str(&command).unwrap();
            assert_eq!(
                command.get("method").and_then(Value::as_str),
                Some("Runtime.evaluate")
            );
            let id = command.get("id").and_then(Value::as_u64).unwrap();
            socket
                .send(Message::Text(
                    json!({"id": id, "result": {"result": {"value": "pending"}}})
                        .to_string()
                        .into(),
                ))
                .unwrap();
            observations += 1;
        }
        observations
    });

    let monitor_alive = alive.clone();
    let monitor = thread::spawn(move || {
        let mut failure = None;
        let result = super::monitor(port, &monitor_alive, |error| {
            failure = Some(error.to_string());
        });
        (result, failure)
    });

    // Existing transport grace is three seconds. A positive Pending result is
    // state, not a transport failure or a mask failure, so it has no timer.
    thread::sleep(Duration::from_millis(3400));
    alive.store(false, Ordering::Release);
    let (result, failure) = monitor.join().unwrap();
    let observations = server.join().unwrap();

    assert!(
        observations >= 10,
        "fixture must span the existing transport grace"
    );
    assert!(
        result.is_ok(),
        "pending mask state must not fail supervision: {failure:?}"
    );
    assert!(
        failure.is_none(),
        "pending mask state must not request shutdown"
    );
}

fn accept_while_alive(listener: &TcpListener, alive: &AtomicBool) -> Option<TcpStream> {
    while alive.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return Some(stream);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("accept loopback CDP request: {error}"),
        }
    }
    None
}

fn read_request_path(stream: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut byte = [0u8; 1];
    while !request.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        request.push(byte[0]);
    }
    String::from_utf8(request)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_string()
}

fn write_json(stream: &mut TcpStream, value: &Value) {
    let body = value.to_string();
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    stream.flush().unwrap();
}
