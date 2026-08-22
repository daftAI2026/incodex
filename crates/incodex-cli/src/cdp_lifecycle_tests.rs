use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::Message;

use super::super::monitor_primary_target;

fn read_request_path(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut request = [0_u8; 2048];
    let read = stream.read(&mut request).unwrap();
    let request = String::from_utf8_lossy(&request[..read]);
    request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap()
        .to_string()
}

fn write_json(stream: &mut TcpStream, value: &Value) {
    let body = value.to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).unwrap();
}

fn write_error(stream: &mut TcpStream) {
    stream
        .write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .unwrap();
}

fn page(port: u16, id: &str, url: &str) -> Value {
    json!({
        "id": id,
        "type": "page",
        "url": url,
        "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/{id}")
    })
}

fn overlay(port: u16) -> Value {
    page(
        port,
        "overlay",
        "app://-/index.html?initialRoute=%2Favatar-overlay",
    )
}

fn accept_until(listener: &TcpListener, deadline: Instant) -> Option<TcpStream> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                return Some(stream);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("mock CDP listener failed: {error}"),
        }
    }
}

#[test]
fn lifecycle_adopts_a_replacement_primary_after_a_transient_gap() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut list_requests = 0;
        let mut browser_close_requested = false;

        while let Some(mut stream) = accept_until(&listener, deadline) {
            match read_request_path(&mut stream).as_str() {
                "/json/list" => {
                    list_requests += 1;
                    match list_requests {
                        1 => write_json(&mut stream, &json!([overlay(port)])),
                        2 | 3 => write_json(
                            &mut stream,
                            &json!([
                                page(port, "replacement", "app://-/index.html"),
                                overlay(port)
                            ]),
                        ),
                        _ => write_error(&mut stream),
                    }
                }
                "/json" => {
                    write_error(&mut stream);
                    if list_requests >= 4 {
                        break;
                    }
                }
                "/json/version" => {
                    browser_close_requested = true;
                    write_json(&mut stream, &json!({}));
                    break;
                }
                path => panic!("unexpected mock CDP path: {path}"),
            }
        }

        (list_requests, browser_close_requested)
    });

    monitor_primary_target(port, "original");
    let (list_requests, browser_close_requested) = server.join().unwrap();

    assert!(
        list_requests >= 3,
        "the replacement primary must remain under lifecycle monitoring"
    );
    assert!(
        !browser_close_requested,
        "a one-poll target handoff must not close the isolated browser"
    );
}

#[test]
fn lifecycle_requires_consecutive_primary_absence_before_closing() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut list_requests = 0;
        let mut list_requests_at_close = None;
        let mut close_received = false;

        while let Some(mut stream) = accept_until(&listener, deadline) {
            match read_request_path(&mut stream).as_str() {
                "/json/list" => {
                    list_requests += 1;
                    write_json(&mut stream, &json!([overlay(port)]));
                }
                "/json/version" => {
                    list_requests_at_close = Some(list_requests);
                    write_json(
                        &mut stream,
                        &json!({
                            "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/browser/close")
                        }),
                    );
                    let Some(stream) = accept_until(&listener, deadline) else {
                        break;
                    };
                    let mut socket = tungstenite::accept(stream).unwrap();
                    let Message::Text(command) = socket.read().unwrap() else {
                        panic!("Browser.close must be a text CDP command");
                    };
                    close_received = serde_json::from_str::<Value>(&command)
                        .ok()
                        .and_then(|value| value.get("method").cloned())
                        == Some(json!("Browser.close"));
                    break;
                }
                path => panic!("unexpected mock CDP path: {path}"),
            }
        }

        (list_requests_at_close, close_received)
    });

    monitor_primary_target(port, "main");
    let (list_requests_at_close, close_received) = server.join().unwrap();

    assert!(
        close_received,
        "confirmed primary loss must close the browser"
    );
    assert!(
        list_requests_at_close.unwrap_or_default() >= 2,
        "one missing poll is a target handoff race, not a confirmed close"
    );
}

#[test]
fn lifecycle_retries_browser_close_after_a_transient_cdp_failure() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut version_requests = 0;
        let mut close_received = false;

        while let Some(mut stream) = accept_until(&listener, deadline) {
            match read_request_path(&mut stream).as_str() {
                "/json/list" => write_json(&mut stream, &json!([overlay(port)])),
                "/json/version" => {
                    version_requests += 1;
                    write_json(
                        &mut stream,
                        &json!({
                            "webSocketDebuggerUrl": format!(
                                "ws://127.0.0.1:{port}/devtools/browser/{version_requests}"
                            )
                        }),
                    );
                    let Some(stream) = accept_until(&listener, deadline) else {
                        break;
                    };
                    if version_requests == 1 {
                        drop(stream);
                        continue;
                    }
                    let mut socket = tungstenite::accept(stream).unwrap();
                    let Message::Text(command) = socket.read().unwrap() else {
                        panic!("Browser.close must be a text CDP command");
                    };
                    close_received = serde_json::from_str::<Value>(&command)
                        .ok()
                        .and_then(|value| value.get("method").cloned())
                        == Some(json!("Browser.close"));
                    break;
                }
                path => panic!("unexpected mock CDP path: {path}"),
            }
        }

        (version_requests, close_received)
    });

    monitor_primary_target(port, "main");
    let (version_requests, close_received) = server.join().unwrap();

    assert!(
        version_requests >= 2,
        "a transient Browser.close failure must be retried"
    );
    assert!(close_received, "the retry must deliver Browser.close");
}
