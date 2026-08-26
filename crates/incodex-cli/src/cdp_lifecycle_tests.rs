use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::Message;

use super::{monitor_primary_target, start_profile_mask_signal_monitor};

#[test]
#[cfg(target_os = "windows")]
fn persistent_cdp_loss_requests_windows_job_shutdown() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let alive = Arc::new(AtomicBool::new(true));
    let close_requested = Arc::new(AtomicBool::new(false));
    let cdp_failed = Arc::new(AtomicBool::new(false));

    let monitor = super::start_lifecycle_signal_monitor(
        port,
        "main".to_string(),
        alive.clone(),
        close_requested.clone(),
        cdp_failed.clone(),
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    while !close_requested.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    alive.store(false, Ordering::Release);
    monitor.join().unwrap();
    assert!(!close_requested.load(Ordering::Acquire));
    assert!(cdp_failed.load(Ordering::Acquire));
}

#[test]
#[cfg(target_os = "windows")]
fn windows_lifecycle_survives_one_failed_poll_before_a_normal_close() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut failed_poll = false;
        let mut healthy_polls = 0;
        while let Some(mut stream) = accept_until(&listener, deadline) {
            match read_request_path(&mut stream).as_str() {
                "/json/list" if !failed_poll => write_error(&mut stream),
                "/json" if !failed_poll => {
                    failed_poll = true;
                    write_error(&mut stream);
                }
                "/json/list" => {
                    healthy_polls += 1;
                    if healthy_polls == 1 {
                        write_json(
                            &mut stream,
                            &json!([page(port, "main", "app://-/index.html")]),
                        );
                    } else {
                        write_json(&mut stream, &json!([overlay(port)]));
                    }
                }
                path => panic!("unexpected mock CDP path: {path}"),
            }
            if healthy_polls >= 3 {
                break;
            }
        }
    });
    let alive = Arc::new(AtomicBool::new(true));
    let close_requested = Arc::new(AtomicBool::new(false));
    let cdp_failed = Arc::new(AtomicBool::new(false));
    let monitor = super::start_lifecycle_signal_monitor(
        port,
        "main".to_string(),
        alive.clone(),
        close_requested.clone(),
        cdp_failed.clone(),
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    while !close_requested.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    alive.store(false, Ordering::Release);
    monitor.join().unwrap();
    server.join().unwrap();
    assert!(close_requested.load(Ordering::Acquire));
    assert!(!cdp_failed.load(Ordering::Acquire));
}

#[test]
#[cfg(target_os = "windows")]
fn windows_lifecycle_closes_instead_of_adopting_an_uninjected_replacement() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut polls = 0;
        while let Some(mut stream) = accept_until(&listener, deadline) {
            assert_eq!(read_request_path(&mut stream), "/json/list");
            polls += 1;
            write_json(
                &mut stream,
                &json!([page(port, "replacement", "app://-/index.html")]),
            );
            if polls >= 2 {
                break;
            }
        }
    });
    let alive = Arc::new(AtomicBool::new(true));
    let close_requested = Arc::new(AtomicBool::new(false));
    let cdp_failed = Arc::new(AtomicBool::new(false));
    let monitor = super::start_lifecycle_signal_monitor(
        port,
        "main".to_string(),
        alive.clone(),
        close_requested.clone(),
        cdp_failed.clone(),
    );

    let deadline = Instant::now() + Duration::from_secs(3);
    while !close_requested.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    alive.store(false, Ordering::Release);
    monitor.join().unwrap();
    server.join().unwrap();
    assert!(close_requested.load(Ordering::Acquire));
    assert!(!cdp_failed.load(Ordering::Acquire));
}

fn monitor_while_server<T>(port: u16, primary_target_id: &str, server: thread::JoinHandle<T>) -> T {
    let process_alive = Arc::new(AtomicBool::new(true));
    let monitor_process_alive = process_alive.clone();
    let primary_target_id = primary_target_id.to_string();
    let monitor = thread::spawn(move || {
        monitor_primary_target(port, &primary_target_id, &monitor_process_alive, || {
            let _ = super::close_browser_with_retries(port);
            false
        })
    });
    let result = server.join().unwrap();
    process_alive.store(false, Ordering::Release);
    monitor.join().unwrap();
    result
}

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
        let mut list_requests_at_close = None;
        let mut close_received = false;

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
                        _ => write_json(&mut stream, &json!([overlay(port)])),
                    }
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

    let (list_requests_at_close, close_received) = monitor_while_server(port, "original", server);

    assert!(
        list_requests_at_close.unwrap_or_default() >= 5,
        "the replacement primary must be adopted and observed before its later close"
    );
    assert!(
        close_received,
        "the replacement primary's normal disappearance must close the browser"
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

    let (list_requests_at_close, close_received) = monitor_while_server(port, "main", server);

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

    let (version_requests, close_received) = monitor_while_server(port, "main", server);

    assert!(
        version_requests >= 2,
        "a transient Browser.close failure must be retried"
    );
    assert!(close_received, "the retry must deliver Browser.close");
}

#[test]
fn lifecycle_survives_a_transient_target_list_failure() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut failed_first_poll = false;
        let mut successful_list_requests = 0;
        let mut close_received = false;

        while let Some(mut stream) = accept_until(&listener, deadline) {
            match read_request_path(&mut stream).as_str() {
                "/json/list" if !failed_first_poll => {
                    failed_first_poll = true;
                    write_error(&mut stream);
                }
                "/json" if successful_list_requests == 0 => write_error(&mut stream),
                "/json/list" => {
                    successful_list_requests += 1;
                    if successful_list_requests == 1 {
                        write_json(
                            &mut stream,
                            &json!([page(port, "main", "app://-/index.html")]),
                        );
                    } else {
                        write_json(&mut stream, &json!([overlay(port)]));
                    }
                }
                "/json/version" => {
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

        (successful_list_requests, close_received)
    });

    let (successful_list_requests, close_received) = monitor_while_server(port, "main", server);

    assert!(
        successful_list_requests >= 3,
        "the monitor must resume after a transient target-list failure"
    );
    assert!(
        close_received,
        "closing the recovered primary must still close the isolated browser"
    );
}

#[test]
fn lifecycle_reissues_browser_close_while_the_browser_is_still_alive() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut close_commands = 0;

        while let Some(mut stream) = accept_until(&listener, deadline) {
            match read_request_path(&mut stream).as_str() {
                "/json/list" => write_json(&mut stream, &json!([overlay(port)])),
                "/json/version" => {
                    write_json(
                        &mut stream,
                        &json!({
                            "webSocketDebuggerUrl": format!(
                                "ws://127.0.0.1:{port}/devtools/browser/{}",
                                close_commands + 1
                            )
                        }),
                    );
                    let Some(stream) = accept_until(&listener, deadline) else {
                        break;
                    };
                    let mut socket = tungstenite::accept(stream).unwrap();
                    let Message::Text(command) = socket.read().unwrap() else {
                        panic!("Browser.close must be a text CDP command");
                    };
                    let is_close = serde_json::from_str::<Value>(&command)
                        .ok()
                        .and_then(|value| value.get("method").cloned())
                        == Some(json!("Browser.close"));
                    assert!(is_close, "expected Browser.close, got {command}");
                    close_commands += 1;
                    if close_commands == 2 {
                        break;
                    }
                }
                path => panic!("unexpected mock CDP path: {path}"),
            }
        }

        close_commands
    });

    let close_commands = monitor_while_server(port, "main", server);

    assert_eq!(
        close_commands, 2,
        "sending Browser.close is not proof of exit; a live browser must be checked and closed again"
    );
}

#[test]
fn lifecycle_recovers_after_three_complete_target_list_failures() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut failed_polls = 0;
        let mut successful_list_requests = 0;
        let mut close_received = false;

        while let Some(mut stream) = accept_until(&listener, deadline) {
            match read_request_path(&mut stream).as_str() {
                "/json/list" if failed_polls < 3 => write_error(&mut stream),
                "/json" if failed_polls < 3 => {
                    failed_polls += 1;
                    write_error(&mut stream);
                }
                "/json/list" => {
                    successful_list_requests += 1;
                    if successful_list_requests == 1 {
                        write_json(
                            &mut stream,
                            &json!([page(port, "main", "app://-/index.html")]),
                        );
                    } else {
                        write_json(&mut stream, &json!([overlay(port)]));
                    }
                }
                "/json/version" => {
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

        (failed_polls, successful_list_requests, close_received)
    });

    let (failed_polls, successful_list_requests, close_received) =
        monitor_while_server(port, "main", server);

    assert_eq!(failed_polls, 3, "the mock must exercise three failed polls");
    assert!(
        successful_list_requests >= 3,
        "the monitor must remain alive long enough to observe recovery and the later close"
    );
    assert!(
        close_received,
        "the recovered primary's disappearance must still close the browser"
    );
}

#[test]
fn lifecycle_reissues_close_after_post_close_cdp_recovery() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut failed_polls_after_first_close = 0;
        let mut close_commands = 0;

        while let Some(mut stream) = accept_until(&listener, deadline) {
            match read_request_path(&mut stream).as_str() {
                "/json/list" if close_commands == 1 && failed_polls_after_first_close < 3 => {
                    write_error(&mut stream);
                }
                "/json" if close_commands == 1 && failed_polls_after_first_close < 3 => {
                    failed_polls_after_first_close += 1;
                    write_error(&mut stream);
                }
                "/json/list" => write_json(&mut stream, &json!([overlay(port)])),
                "/json/version" => {
                    write_json(
                        &mut stream,
                        &json!({
                            "webSocketDebuggerUrl": format!(
                                "ws://127.0.0.1:{port}/devtools/browser/{}",
                                close_commands + 1
                            )
                        }),
                    );
                    let Some(stream) = accept_until(&listener, deadline) else {
                        break;
                    };
                    let mut socket = tungstenite::accept(stream).unwrap();
                    let Message::Text(command) = socket.read().unwrap() else {
                        panic!("Browser.close must be a text CDP command");
                    };
                    let is_close = serde_json::from_str::<Value>(&command)
                        .ok()
                        .and_then(|value| value.get("method").cloned())
                        == Some(json!("Browser.close"));
                    assert!(is_close, "expected Browser.close, got {command}");
                    close_commands += 1;
                    if close_commands == 2 {
                        break;
                    }
                }
                path => panic!("unexpected mock CDP path: {path}"),
            }
        }

        (failed_polls_after_first_close, close_commands)
    });

    let (failed_polls_after_first_close, close_commands) =
        monitor_while_server(port, "main", server);

    assert_eq!(
        failed_polls_after_first_close, 3,
        "the mock must exercise three complete post-close polling failures"
    );
    assert_eq!(
        close_commands, 2,
        "temporary post-close CDP loss is not proof of exit; recovered auxiliary targets require another close"
    );
}

#[test]
fn persistent_profile_mask_health_failure_is_reported_to_the_parent() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut health_probes = 0;

        for _ in 0..2 {
            let Some(mut stream) = accept_until(&listener, deadline) else {
                return (health_probes, false);
            };
            assert_eq!(read_request_path(&mut stream), "/json/list");
            write_json(
                &mut stream,
                &json!([page(port, "main", "app://-/index.html")]),
            );

            let Some(stream) = accept_until(&listener, deadline) else {
                return (health_probes, false);
            };
            let mut socket = tungstenite::accept(stream).unwrap();
            let Message::Text(command) = socket.read().unwrap() else {
                panic!("profile health probe must be a text CDP command");
            };
            let command: Value = serde_json::from_str(&command).unwrap();
            assert_eq!(
                command.get("method").and_then(Value::as_str),
                Some("Runtime.evaluate")
            );
            assert_eq!(
                command
                    .pointer("/params/expression")
                    .and_then(Value::as_str),
                Some(if cfg!(target_os = "windows") {
                    "window.__incodexRefreshProfileMaskHealth?.() === true"
                } else {
                    "window.__incodexProfileMaskHealth === true"
                })
            );
            let id = command.get("id").and_then(Value::as_u64).unwrap();
            socket
                .send(Message::Text(
                    json!({"id": id, "result": {"result": {"value": false}}})
                        .to_string()
                        .into(),
                ))
                .unwrap();
            health_probes += 1;
        }

        let direct_close_attempted =
            accept_until(&listener, Instant::now() + Duration::from_millis(600))
                .map(|mut stream| read_request_path(&mut stream) == "/json/version")
                .unwrap_or(false);
        (health_probes, direct_close_attempted)
    });

    let process_alive = Arc::new(AtomicBool::new(true));
    let cdp_failed = Arc::new(AtomicBool::new(false));
    let monitor =
        start_profile_mask_signal_monitor(port, process_alive.clone(), cdp_failed.clone(), |_| {
            Ok(())
        });
    let deadline = Instant::now() + Duration::from_secs(3);
    while !cdp_failed.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    process_alive.store(false, Ordering::Release);
    monitor.join().unwrap();
    let (health_probes, direct_close_attempted) = server.join().unwrap();

    assert_eq!(health_probes, 2, "one transient failure may recover");
    assert!(
        cdp_failed.load(Ordering::Acquire),
        "the parent lifecycle must receive the failure"
    );
    assert!(
        !direct_close_attempted,
        "the health monitor must leave termination and session cleanup to the parent"
    );
}

#[test]
#[cfg(target_os = "windows")]
fn missing_profile_target_is_left_to_the_primary_lifecycle_monitor() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let mut stream = accept_until(&listener, Instant::now() + Duration::from_secs(2))
                .expect("profile monitor did not poll targets");
            assert_eq!(read_request_path(&mut stream), "/json/list");
            write_json(&mut stream, &json!([]));
        }
    });
    let process_alive = Arc::new(AtomicBool::new(true));
    let cdp_failed = Arc::new(AtomicBool::new(false));
    let monitor =
        start_profile_mask_signal_monitor(port, process_alive.clone(), cdp_failed.clone(), |_| {
            Ok(())
        });

    server.join().unwrap();
    thread::sleep(Duration::from_millis(250));
    process_alive.store(false, Ordering::Release);
    monitor.join().unwrap();

    assert!(
        !cdp_failed.load(Ordering::Acquire),
        "target loss belongs to the primary lifecycle monitor, not mask health"
    );
}

#[test]
#[cfg(target_os = "windows")]
fn persistent_profile_probe_transport_failure_is_reported_to_the_parent() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        for _ in 0..2 {
            let mut stream = accept_until(&listener, Instant::now() + Duration::from_secs(2))
                .expect("profile monitor did not poll targets");
            assert_eq!(read_request_path(&mut stream), "/json/list");
            write_json(
                &mut stream,
                &json!([page(port, "main", "app://-/index.html")]),
            );

            let stream = accept_until(&listener, Instant::now() + Duration::from_secs(2))
                .expect("profile monitor did not connect to the target");
            drop(stream);
        }
    });
    let process_alive = Arc::new(AtomicBool::new(true));
    let cdp_failed = Arc::new(AtomicBool::new(false));
    let monitor =
        start_profile_mask_signal_monitor(port, process_alive.clone(), cdp_failed.clone(), |_| {
            Ok(())
        });

    server.join().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !cdp_failed.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    process_alive.store(false, Ordering::Release);
    monitor.join().unwrap();

    assert!(
        cdp_failed.load(Ordering::Acquire),
        "a present target with a persistently broken transport is a mask-health failure"
    );
}

#[test]
#[cfg(target_os = "windows")]
fn missing_profile_target_does_not_erase_a_confirmed_mask_failure() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        for round in 0..3 {
            let mut stream = accept_until(&listener, Instant::now() + Duration::from_secs(2))
                .expect("profile monitor did not poll targets");
            assert_eq!(read_request_path(&mut stream), "/json/list");
            if round == 1 {
                write_json(&mut stream, &json!([]));
                continue;
            }
            write_json(
                &mut stream,
                &json!([page(port, "main", "app://-/index.html")]),
            );

            let stream = accept_until(&listener, Instant::now() + Duration::from_secs(2))
                .expect("profile monitor did not connect to the target");
            let mut socket = tungstenite::accept(stream).unwrap();
            let Message::Text(command) = socket.read().unwrap() else {
                panic!("profile health probe must be a text CDP command");
            };
            let command: Value = serde_json::from_str(&command).unwrap();
            let id = command.get("id").and_then(Value::as_u64).unwrap();
            socket
                .send(Message::Text(
                    json!({"id": id, "result": {"result": {"value": false}}})
                        .to_string()
                        .into(),
                ))
                .unwrap();
        }
    });
    let process_alive = Arc::new(AtomicBool::new(true));
    let cdp_failed = Arc::new(AtomicBool::new(false));
    let monitor =
        start_profile_mask_signal_monitor(port, process_alive.clone(), cdp_failed.clone(), |_| {
            Ok(())
        });

    server.join().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !cdp_failed.load(Ordering::Acquire) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    process_alive.store(false, Ordering::Release);
    monitor.join().unwrap();

    assert!(
        cdp_failed.load(Ordering::Acquire),
        "deferred target loss must not reset an already confirmed unhealthy mask poll"
    );
}
