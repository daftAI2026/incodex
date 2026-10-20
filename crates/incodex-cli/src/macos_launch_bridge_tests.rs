/*
 * [INPUT]: 依赖生产CDP转接、当前测试进程与自有loopback socket。
 * [OUTPUT]: 验证传输归属、动作提交顺序及精确ACK拒绝。
 * [POS]: launch桥接的无窗回归；不代表真实官方UI验收。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;
use crate::macos_launch_process::NormalHost;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tungstenite::{accept, Message};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvaluationKind {
    Stage,
    RendererAck,
    ActionCommit,
}

struct EvaluationStep {
    kind: EvaluationKind,
    value: Value,
}

struct WebSocketFixture {
    listener: TcpListener,
    host: NormalHost,
    port: u16,
    websocket_url: String,
}

impl WebSocketFixture {
    fn new() -> Self {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let host = NormalHost::current_process_for_test(port).unwrap();
        host.wait_for_test_listener_owner(port).unwrap();
        Self {
            listener,
            host,
            port,
            websocket_url: format!("ws://127.0.0.1:{port}/devtools/page/fixture"),
        }
    }

    fn serve(&self, steps: Vec<EvaluationStep>) -> JoinHandle<Vec<EvaluationKind>> {
        let listener = self.listener.try_clone().unwrap();
        thread::spawn(move || {
            let mut observed = Vec::new();
            for step in steps {
                let stream = accept_before(&listener);
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut socket = accept(stream).unwrap();
                socket.get_ref().set_nonblocking(false).unwrap();
                socket
                    .get_ref()
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let request = loop {
                    match socket.read().unwrap() {
                        Message::Text(text) => break serde_json::from_str::<Value>(&text).unwrap(),
                        Message::Ping(bytes) => socket.send(Message::Pong(bytes)).unwrap(),
                        _ => {}
                    }
                };
                assert_eq!(request["method"], "Runtime.evaluate");
                let expression = request["params"]["expression"].as_str().unwrap();
                match step.kind {
                    EvaluationKind::Stage => {
                        assert!(expression.contains("controller.stage"));
                        assert!(expression.contains("window.__incodexRendererRequest"));
                        assert!(expression.contains("fixture action source"));
                        assert!(expression.contains("fixture injector source"));
                    }
                    EvaluationKind::RendererAck => {
                        assert!(expression.contains("window.__incodexRendererGeneration"));
                        assert!(expression.contains("data-incodex-privacy-toggle"));
                    }
                    EvaluationKind::ActionCommit => {
                        assert!(expression.contains("commitStaged"));
                        assert!(expression.contains("action-hash"));
                    }
                }
                observed.push(step.kind);
                let response = json!({
                    "id": request["id"],
                    "result": { "result": { "value": step.value } }
                });
                socket
                    .send(Message::Text(response.to_string().into()))
                    .unwrap();
            }
            observed
        })
    }
}

fn accept_before(listener: &TcpListener) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                // macOS接受的socket继承nonblocking；握手与HTTP读取须有界阻塞。
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for CDP fixture client"
                );
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("fixture accept failed: {error}"),
        }
    }
}

fn read_http_headers(stream: &mut TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut chunk = [0_u8; 512];
    loop {
        let read = stream.read(&mut chunk).unwrap();
        assert_ne!(read, 0, "fixture client closed before HTTP headers arrived");
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return request;
        }
        assert!(request.len() < 8192, "fixture HTTP headers exceeded limit");
    }
}

fn generation() -> UiGeneration {
    UiGeneration {
        release: "fixture-release".into(),
        source: "/* fixture injector source */ window.__incodexRendererRequest = {};".into(),
        action_source: "/* fixture action source */".into(),
        files: [
            ("incodex-inject.js".into(), "injector-hash".into()),
            ("incodex-main-actions.cjs".into(), "action-hash".into()),
        ]
        .into_iter()
        .collect(),
    }
}

fn stage_ack(action_id: &str) -> Value {
    json!({"actions":{"protocol":1,"id":action_id}})
}

fn renderer_ack(injector_id: &str) -> Value {
    json!({
        "ui":{"protocol":1,"id":injector_id,"restartRequired":false},
        "button":true
    })
}

fn step(kind: EvaluationKind, value: Value) -> EvaluationStep {
    EvaluationStep { kind, value }
}

#[test]
fn real_loopback_transport_waits_for_exact_renderer_ack_before_action_commit() {
    let fixture = WebSocketFixture::new();
    let generation = generation();
    let server = fixture.serve(vec![
        step(EvaluationKind::Stage, stage_ack("action-hash")),
        step(
            EvaluationKind::RendererAck,
            renderer_ack("old-injector-hash"),
        ),
        step(EvaluationKind::RendererAck, renderer_ack("injector-hash")),
        step(EvaluationKind::ActionCommit, stage_ack("action-hash")),
    ]);
    let page = [("fixture-page", fixture.websocket_url.as_str())];

    assert!(apply_generation(&fixture.host, fixture.port, &page, &generation, false).unwrap());
    assert!(apply_generation(&fixture.host, fixture.port, &page, &generation, true).unwrap());
    assert_eq!(
        server.join().unwrap(),
        [
            EvaluationKind::Stage,
            EvaluationKind::RendererAck,
            EvaluationKind::RendererAck,
            EvaluationKind::ActionCommit,
        ]
    );
}

#[test]
fn stage_and_commit_reject_wrong_action_acknowledgements() {
    let generation = generation();
    let stage_fixture = WebSocketFixture::new();
    let stage_server = stage_fixture.serve(vec![step(
        EvaluationKind::Stage,
        stage_ack("wrong-action-hash"),
    )]);
    let page = [("fixture-page", stage_fixture.websocket_url.as_str())];
    assert!(!apply_generation(
        &stage_fixture.host,
        stage_fixture.port,
        &page,
        &generation,
        false
    )
    .unwrap());
    assert_eq!(stage_server.join().unwrap(), [EvaluationKind::Stage]);

    let commit_fixture = WebSocketFixture::new();
    let commit_server = commit_fixture.serve(vec![step(
        EvaluationKind::ActionCommit,
        stage_ack("wrong-action-hash"),
    )]);
    let page = [("fixture-page", commit_fixture.websocket_url.as_str())];
    assert!(!apply_generation(
        &commit_fixture.host,
        commit_fixture.port,
        &page,
        &generation,
        true
    )
    .unwrap());
    assert_eq!(
        commit_server.join().unwrap(),
        [EvaluationKind::ActionCommit]
    );
}

#[test]
fn target_discovery_reads_a_real_loopback_http_response() {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let server = thread::spawn(move || {
        let mut stream = accept_before(&listener);
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let request = read_http_headers(&mut stream);
        let request = String::from_utf8_lossy(&request);
        assert!(request.starts_with("GET /json/list HTTP/1.1"));
        let body = json!([{
            "id":"fixture-page",
            "type":"page",
            "url":"app://-/index.html",
            "webSocketDebuggerUrl":format!("ws://127.0.0.1:{port}/devtools/page/fixture-page")
        }])
        .to_string();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });

    let targets = list_targets(port).unwrap();
    server.join().unwrap();
    assert_eq!(targets.len(), 1);
    assert!(is_primary_codex_page(&targets[0]));
    assert_eq!(targets[0].id, "fixture-page");
}

#[test]
fn established_target_discovery_error_keeps_page_and_pending_request_for_retry() {
    let mut pages = HashMap::from([("live-page".to_string(), ())]);
    let request = NativeOpenBridgeRequest {
        request_id: "incodex-request-1234".into(),
        execution_context_id: 7,
        source_bounds: None,
        runtime_release: None,
    };
    let mut pending = VecDeque::from([Pending {
        page: "live-page".into(),
        request,
    }]);

    let iteration = reconcile_target_discovery(
        Err("temporary loopback HTTP failure".into()),
        true,
        false,
        false,
        &mut pages,
        &mut pending,
    );

    assert!(
        matches!(iteration, Ok(DiscoveryIteration::Retry)),
        "a transient read failure after readiness must retry without retiring the bridge"
    );
    assert!(pages.contains_key("live-page"));
    assert_eq!(pending.len(), 1);
}

#[test]
fn empty_target_snapshot_does_not_retire_an_open_page_or_its_request() {
    let mut pages = HashMap::from([("live-page".to_string(), ())]);
    let request = NativeOpenBridgeRequest {
        request_id: "incodex-request-1234".into(),
        execution_context_id: 7,
        source_bounds: None,
        runtime_release: None,
    };
    let mut pending = VecDeque::from([Pending {
        page: "live-page".into(),
        request,
    }]);

    let iteration = reconcile_target_discovery(
        Ok(Vec::new()),
        true,
        false,
        false,
        &mut pages,
        &mut pending,
    )
    .unwrap();

    assert!(
        matches!(iteration, DiscoveryIteration::Retry),
        "an empty snapshot must not evict a page whose CDP channel remains open"
    );
    assert!(pages.contains_key("live-page"));
    assert_eq!(pending.len(), 1);
}

#[test]
fn binding_event_parser_preserves_request_identity_and_rejects_untrusted_shape() {
    let event = json!({
        "method":"Runtime.bindingCalled",
        "params":{
            "name":BINDING,
            "executionContextId":7,
            "payload":json!({
                "action":"open",
                "requestId":"incodex-12345678",
                "sourceBounds":"1,2,800,600",
                "runtimeRelease":"fixture-release"
            }).to_string()
        }
    });
    let request = request_from_event(&event).unwrap();
    assert_eq!(request.request_id, "incodex-12345678");
    assert_eq!(request.execution_context_id, 7);
    assert_eq!(request.source_bounds.as_deref(), Some("1,2,800,600"));
    assert_eq!(request.runtime_release.as_deref(), Some("fixture-release"));

    let mut wrong_binding = event.clone();
    wrong_binding["params"]["name"] = json!("unrelated");
    assert!(request_from_event(&wrong_binding).is_none());

    let mut wrong_action = event.clone();
    wrong_action["params"]["payload"] = json!({"action":"other","requestId":"incodex-12345678"})
        .to_string()
        .into();
    assert!(request_from_event(&wrong_action).is_none());

    let mut wrong_id = event;
    wrong_id["params"]["payload"] = json!({"action":"open","requestId":"incodex-!"})
        .to_string()
        .into();
    assert!(request_from_event(&wrong_id).is_none());
}

#[test]
fn generation_expression_binds_commit_to_the_action_identity() {
    let generation = generation();
    let stage = generation_expression(&generation, false).unwrap();
    let commit = generation_expression(&generation, true).unwrap();
    assert!(stage.contains("injector-hash"));
    assert!(stage.contains("action-hash"));
    assert!(commit.contains("commitStaged(\"action-hash\")"));

    let mut incomplete = generation;
    incomplete.files.remove("incodex-main-actions.cjs");
    assert!(generation_expression(&incomplete, false).is_err());
}

#[test]
fn current_test_process_owns_a_real_established_loopback_tuple() {
    let fixture = WebSocketFixture::new();
    let client = TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, fixture.port)).unwrap();
    let _server = accept_before(&fixture.listener);
    let output = std::process::Command::new("/usr/sbin/lsof")
        .args(["-nP", "-Fpn"])
        .arg(format!("-iTCP@127.0.0.1:{}", fixture.port))
        .args(["-sTCP:ESTABLISHED"])
        .output()
        .unwrap();
    let details = String::from_utf8_lossy(&output.stdout);
    assert!(
        fixture.host.require_connection_owner(&client).is_ok(),
        "lsof returned {} (status {:?}), client {:?} -> {:?}",
        details,
        output.status.code(),
        client.local_addr(),
        client.peer_addr()
    );
}
