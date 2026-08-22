use std::io::Write;
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;

use serde_json::{json, Value};
use tungstenite::Message;

use super::{inject_shared_ui_with_options, ui_ready_expression, InjectionOptions};

fn write_json_response(stream: &mut TcpStream, value: &Value) {
    let body = value.to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).unwrap();
}

#[test]
fn open_selects_the_official_codex_mode_before_injecting_its_ui() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (commands_tx, commands_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut http, _) = listener.accept().unwrap();
        let target = json!([{
            "id": "main",
            "type": "page",
            "url": "app://-/index.html",
            "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/main")
        }]);
        write_json_response(&mut http, &target);

        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        loop {
            let Message::Text(text) = socket.read().unwrap() else {
                continue;
            };
            let command: Value = serde_json::from_str(&text).unwrap();
            commands_tx.send(command.clone()).unwrap();
            let id = command.get("id").and_then(Value::as_u64).unwrap();
            let is_health_probe = command.get("method").and_then(Value::as_str)
                == Some("Runtime.evaluate")
                && command
                    .pointer("/params/expression")
                    .and_then(Value::as_str)
                    == Some(ui_ready_expression());
            let response = if is_health_probe {
                json!({
                    "id": id,
                    "result": {"result": {"value": {"button": true, "banner": true}}}
                })
            } else {
                json!({"id": id, "result": {}})
            };
            socket
                .send(Message::Text(response.to_string().into()))
                .unwrap();
            if is_health_probe {
                break;
            }
        }
    });

    inject_shared_ui_with_options(port, &InjectionOptions::default()).unwrap();
    server.join().unwrap();
    let commands: Vec<Value> = commands_rx.try_iter().collect();
    let mode_keys: Vec<&Value> = commands
        .iter()
        .filter(|command| {
            command.get("method").and_then(Value::as_str) == Some("Input.dispatchKeyEvent")
        })
        .collect();

    assert_eq!(mode_keys.len(), 2, "Codex mode must receive Ctrl+3 down/up");
    assert_eq!(
        mode_keys[0].pointer("/params/type"),
        Some(&json!("rawKeyDown"))
    );
    assert_eq!(mode_keys[0].pointer("/params/modifiers"), Some(&json!(2)));
    assert_eq!(mode_keys[0].pointer("/params/code"), Some(&json!("Digit3")));
    assert_eq!(mode_keys[1].pointer("/params/type"), Some(&json!("keyUp")));

    let first_runtime = commands
        .iter()
        .position(|command| command.get("method") == Some(&json!("Runtime.evaluate")))
        .unwrap();
    let last_mode_key = commands
        .iter()
        .rposition(|command| command.get("method") == Some(&json!("Input.dispatchKeyEvent")))
        .unwrap();
    assert!(
        last_mode_key < first_runtime,
        "official mode selection must run first"
    );
}
