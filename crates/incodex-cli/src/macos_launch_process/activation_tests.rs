/*
 * [INPUT]: 依赖 NormalHost.activate、精确进程身份与 loopback CDP fixture。
 * [OUTPUT]: 验证官方窗口动作必须获得焦点 ACK，失败时保留既有 Host。
 * [POS]: macOS launcher 的激活回归；不启动或修改官方 App。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::NormalHost;
use serde_json::{json, Value};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

#[derive(Clone, Copy)]
enum ReplyMode {
    GenericSuccess,
    FocusFalseThenTrue,
    FocusNeverTrue,
    CloseAfterFalse,
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "incodex-normal-activate-{}",
            incodex_transaction::new_install_id()
        ));
        fs::create_dir_all(&path).expect("create isolated activation fixture");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct ActivationFixture {
    _scratch: Scratch,
    app: PathBuf,
    host: NormalHost,
    _listener_guard: TcpListener,
    server: Option<JoinHandle<Vec<Value>>>,
}

impl ActivationFixture {
    fn new(mode: ReplyMode) -> Self {
        let scratch = Scratch::new();
        let app = scratch.0.join("ChatGPT.app");
        let executable_dir = app.join("Contents/MacOS");
        fs::create_dir_all(&executable_dir).expect("create fake app executable directory");
        fs::write(
            app.join("Contents/Info.plist"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>ChatGPT</string></dict></plist>\n",
        )
        .expect("write fake app Info.plist");
        symlink(
            std::env::current_exe().expect("resolve test executable"),
            executable_dir.join("ChatGPT"),
        )
        .expect("bind fake app identity to this test process");

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback CDP fixture");
        listener
            .set_nonblocking(true)
            .expect("make fixture accepts bounded");
        let port = listener.local_addr().unwrap().port();
        let listener_guard = listener.try_clone().expect("retain listener ownership");
        let host = NormalHost::current_process_for_test(port).expect("capture fixture owner");
        let server = thread::spawn(move || serve_cdp(listener, port, mode));

        Self {
            _scratch: scratch,
            app,
            host,
            _listener_guard: listener_guard,
            server: Some(server),
        }
    }

    fn activate(&self) -> Result<(), String> {
        self.host.activate(&self.app)
    }

    fn finish(&mut self) -> Vec<Value> {
        self.server
            .take()
            .expect("fixture server joins once")
            .join()
            .expect("fixture CDP server completes")
    }
}

#[test]
fn normal_activation_rejects_a_generic_cdp_success_without_focus_evidence() {
    let mut fixture = ActivationFixture::new(ReplyMode::GenericSuccess);
    let activation = fixture.activate();
    let requests = fixture.finish();

    let error = activation.expect_err("a generic CDP success is not a focus ACK");
    assert!(
        error.contains("focus"),
        "unexpected activation error: {error}"
    );
    assert_eq!(requests[0]["method"], "Runtime.evaluate");
}

#[test]
fn normal_activation_uses_official_window_action_then_waits_for_focus() {
    let mut fixture = ActivationFixture::new(ReplyMode::FocusFalseThenTrue);
    let activation = fixture.activate();
    let requests = fixture.finish();

    assert!(
        activation.is_ok(),
        "official focus ACK should succeed: {activation:?}"
    );
    let action = &requests[0];
    assert_eq!(action["method"], "Runtime.evaluate");
    assert_eq!(action["params"]["awaitPromise"], true);
    assert_eq!(action["params"]["returnByValue"], true);
    let expression = action["params"]["expression"]
        .as_str()
        .expect("official action expression");
    assert!(expression.contains("sendMessageFromView"));
    assert!(expression.contains("open-current-main-window"));
    assert!(expression.contains("stealFocus:true"));

    assert!(requests.len() >= 2, "false focus must trigger a probe");
    assert_eq!(requests[1]["method"], "Runtime.evaluate");
    assert!(requests[1]["params"]["expression"]
        .as_str()
        .unwrap()
        .contains("document.hasFocus()"));
}

#[test]
fn activation_timeout_keeps_the_captured_host_and_listener() {
    let mut fixture = ActivationFixture::new(ReplyMode::FocusNeverTrue);
    let activation = fixture.activate();
    let requests = fixture.finish();

    let error = activation.expect_err("false focus probes must time out");
    assert!(
        error.contains("focus"),
        "unexpected activation error: {error}"
    );
    assert!(requests.len() > 1, "focus false must be polled");
    assert!(fixture.host.is_alive().unwrap());
    assert!(fixture.host.validate_app(&fixture.app).unwrap());
}

#[test]
fn lost_connection_owner_guard_keeps_the_captured_host() {
    let mut fixture = ActivationFixture::new(ReplyMode::CloseAfterFalse);
    let activation = fixture.activate();
    let requests = fixture.finish();

    let error = activation.expect_err("a lost CDP owner must fail closed");
    assert!(
        error.contains("cannot identify official Codex established CDP connection owner"),
        "expected the existing connection owner guard, got: {error}"
    );
    assert_eq!(
        requests.len(),
        1,
        "the fixture closes after the first false ACK"
    );
    assert!(fixture.host.is_alive().unwrap());
    assert!(fixture.host.validate_app(&fixture.app).unwrap());
}

fn serve_cdp(listener: TcpListener, port: u16, mode: ReplyMode) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let Some(mut http) = accept_until(&listener, deadline) else {
        return Vec::new();
    };
    let request = read_http_headers(&mut http);
    assert!(
        request.starts_with("GET /json/list "),
        "unexpected CDP path: {request}"
    );
    let body = json!([{
        "id": "primary",
        "type": "page",
        "url": "app://-/index.html",
        "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/primary")
    }]);
    let body = body.to_string();
    write!(
        http,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("write CDP target list");
    http.flush().expect("flush CDP target list");
    drop(http);

    let Some(websocket) = accept_until(&listener, deadline) else {
        return Vec::new();
    };
    let mut websocket = tungstenite::accept(websocket).expect("accept fixture CDP websocket");
    let mut requests = Vec::new();
    while let Some(request) = read_request(&mut websocket) {
        let id = request["id"].as_u64().expect("CDP request id");
        let method = request["method"].as_str().expect("CDP method").to_owned();
        let expression = request["params"]["expression"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        requests.push(request);

        let (response, close) = match mode {
            ReplyMode::GenericSuccess => (json!({ "id": id, "result": {} }), false),
            ReplyMode::FocusFalseThenTrue if expression.contains("sendMessageFromView") => {
                (focus_response(id, false), false)
            }
            ReplyMode::FocusFalseThenTrue => (focus_response(id, true), false),
            ReplyMode::FocusNeverTrue if method == "Runtime.evaluate" => {
                (focus_response(id, false), false)
            }
            ReplyMode::FocusNeverTrue => (json!({ "id": id, "result": {} }), false),
            ReplyMode::CloseAfterFalse if method == "Runtime.evaluate" => {
                (focus_response(id, false), true)
            }
            ReplyMode::CloseAfterFalse => (json!({ "id": id, "result": {} }), true),
        };
        websocket
            .send(Message::Text(response.to_string().into()))
            .expect("write fixture CDP reply");
        if close {
            break;
        }
    }
    requests
}

fn focus_response(id: u64, focused: bool) -> Value {
    json!({
        "id": id,
        "result": {"result": {"type": "boolean", "value": focused}}
    })
}

fn read_request(websocket: &mut WebSocket<TcpStream>) -> Option<Value> {
    loop {
        match websocket.read() {
            Ok(Message::Text(text)) => {
                return Some(serde_json::from_str(&text).expect("parse CDP request"));
            }
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => continue,
        }
    }
}

fn accept_until(listener: &TcpListener, deadline: Instant) -> Option<TcpStream> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Some(stream),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return None;
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("accept fixture CDP connection: {error}"),
        }
    }
}

fn read_http_headers(stream: &mut TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("bound HTTP fixture read");
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 512];
    loop {
        let count = stream.read(&mut buffer).expect("read CDP HTTP request");
        assert_ne!(count, 0, "CDP closed before sending its target request");
        bytes.extend_from_slice(&buffer[..count]);
        assert!(
            bytes.len() <= 8192,
            "CDP request headers exceed fixture bound"
        );
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            return String::from_utf8(bytes).expect("CDP HTTP request is UTF-8");
        }
    }
}
