//! [INPUT]: 依赖私密关闭协议和本机 loopback WebSocket 替身。
//! [OUTPUT]: 验证能力、文档和连接归属均成立后才请求正常 Page.close。
//! [POS]: 原注册连接的关闭回归；命令 ACK 不伪造真实窗口或 session 清理成功。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use super::*;
use std::sync::atomic::AtomicUsize;

const NONCE: &str = "0123456789abcdef0123456789abcdef";

#[test]
fn private_close_requires_exact_capability_and_execution_context() {
    let event = json!({"method":"Runtime.bindingCalled","params":{
        "name":BINDING,"payload":NONCE,"executionContextId":17
    }});
    assert_eq!(close_context(&event, NONCE), Some(17));
    assert_eq!(
        close_context(&event, "ffffffffffffffffffffffffffffffff"),
        None
    );
    for (field, value) in [
        ("name", json!("foreign")),
        ("executionContextId", json!(0)),
        ("executionContextId", json!(-1)),
    ] {
        let mut other = event.clone();
        other["params"][field] = value;
        assert_eq!(close_context(&other, NONCE), None);
    }
}

#[test]
fn private_close_bridge_only_admits_quit_from_the_private_top_document() {
    let source = bridge_source(NONCE).unwrap();
    assert!(source.contains("window!==window.top"));
    assert!(source.contains("window.__incodexIncognito!==true"));
    assert!(source.contains("payload?.action!=='quit'"));
    assert!(source.contains("CLOSE_REQUESTED"));
    assert!(!source.contains("window.close()"));
    assert!(bridge_source("not-a-capability").is_err());
}

fn request(proven: bool, authorized: bool) -> (Result<(), String>, Vec<Value>, usize) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let mut commands = vec![];
        for _ in 0..if !authorized {
            0
        } else if proven {
            2
        } else {
            1
        } {
            let Message::Text(text) = ws.read().unwrap() else {
                panic!("expected CDP command")
            };
            let command: Value = serde_json::from_str(&text).unwrap();
            let result = if command["method"] == "Runtime.evaluate" {
                json!({"result":{"value":proven}})
            } else {
                json!({})
            };
            ws.send(Message::Text(
                json!({"id":command["id"],"result":result})
                    .to_string()
                    .into(),
            ))
            .unwrap();
            commands.push(command);
        }
        commands
    });
    let stream = TcpStream::connect(address).unwrap();
    let (mut socket, _) =
        tungstenite::client(format!("ws://{address}/owned-page"), stream).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let close = PrivateClose {
        nonce: NONCE.into(),
        guard: Arc::new(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            if authorized {
                Ok(())
            } else {
                Err("owner changed".into())
            }
        }),
    };
    let result = close.request(&mut socket, 17);
    drop(socket);
    (result, server.join().unwrap(), calls.load(Ordering::SeqCst))
}

#[test]
fn private_close_revalidates_context_and_owner_before_page_close() {
    let (result, commands, guards) = request(true, true);
    result.unwrap();
    assert_eq!(guards, 2);
    assert_eq!(commands.len(), 2);
    assert_eq!(commands[0]["method"], "Runtime.evaluate");
    assert_eq!(commands[0]["params"]["contextId"], 17);
    assert_eq!(commands[1]["method"], "Page.close");
}

#[test]
fn private_close_does_not_close_a_foreign_document_or_changed_owner() {
    for (proven, authorized, expected) in [(false, true, 1), (true, false, 0)] {
        let (result, commands, _) = request(proven, authorized);
        assert!(result.is_err());
        assert_eq!(commands.len(), expected);
        assert!(commands.iter().all(|c| c["method"] != "Page.close"));
    }
}

#[test]
fn retained_registration_dispatches_private_quit_instead_of_discarding_it() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let completed = Arc::new(AtomicBool::new(false));
    let done = completed.clone();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        ws.send(Message::Text(
            json!({"method":"Runtime.bindingCalled","params":{
                "name":BINDING,"payload":NONCE,"executionContextId":17
            }})
            .to_string()
            .into(),
        ))
        .unwrap();
        let mut methods = vec![];
        for _ in 0..2 {
            let Message::Text(text) = ws.read().unwrap() else {
                panic!("expected close protocol")
            };
            let command: Value = serde_json::from_str(&text).unwrap();
            let method = command["method"].as_str().unwrap().to_string();
            let result = if method == "Runtime.evaluate" {
                json!({"result":{"value":true}})
            } else {
                json!({})
            };
            ws.send(Message::Text(
                json!({"id":command["id"],"result":result})
                    .to_string()
                    .into(),
            ))
            .unwrap();
            methods.push(method);
        }
        done.store(true, Ordering::Release);
        methods
    });
    let (socket, _) = tungstenite::client(
        format!("ws://{address}/owned"),
        TcpStream::connect(address).unwrap(),
    )
    .unwrap();
    let mut registration = CdpRegistration {
        socket,
        close: Some(PrivateClose {
            nonce: NONCE.into(),
            guard: Arc::new(|_| Ok(())),
        }),
    };
    registration.socket.get_mut().set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !completed.load(Ordering::Acquire) && Instant::now() < deadline {
        registration.drain();
        thread::sleep(Duration::from_millis(5));
    }
    drop(registration);
    assert_eq!(
        server
            .join()
            .expect("retained connection must dispatch the quit event"),
        ["Runtime.evaluate", "Page.close"]
    );
}
