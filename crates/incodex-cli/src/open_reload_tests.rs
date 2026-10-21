//! [INPUT]: 依赖open注入worker、真实loopback CDP与合成页面target。
//! [OUTPUT]: 验证document reload后保留会话注册，同target重连重新注册。
//! [POS]: open生命周期回归；只模拟CDP协议，不启动官方Host或授权API。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use super::super::process::start_injection_worker;
use super::super::InjectionStatus;
use crate::cdp::InjectionOptions;
use crate::profile_mask::{ProfileAvatar, ProfileMask};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tungstenite::Message;

const TARGET_ID: &str = "reload-main";

#[derive(Clone, Copy)]
enum Control {
    Reload,
    Disconnect,
}

struct Connection {
    id: u64,
    control: mpsc::Sender<Control>,
}

#[derive(Default)]
struct ObservedSession {
    methods: Vec<String>,
    expressions: Vec<String>,
    reload_replays: usize,
    closed: bool,
}

#[derive(Default)]
struct FixtureState {
    sessions: Mutex<HashMap<u64, ObservedSession>>,
    controls: Mutex<Vec<mpsc::Sender<Control>>>,
    mode_available: AtomicBool,
}

struct ReloadFixture {
    port: u16,
    stop: Arc<AtomicBool>,
    state: Arc<FixtureState>,
    connections: mpsc::Receiver<Connection>,
    server: Option<JoinHandle<()>>,
}

impl ReloadFixture {
    fn start() -> Self {
        Self::start_with_options(InjectionOptions::default())
    }

    fn start_with_options(options: InjectionOptions) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(FixtureState {
            mode_available: AtomicBool::new(true),
            ..FixtureState::default()
        });
        let worker_stop = stop.clone();
        let worker_state = state.clone();
        let health_expression = crate::cdp::ui_ready_expression_for_options(&options);
        let (connection_tx, connections) = mpsc::channel();
        let server = thread::spawn(move || {
            let mut websocket_workers = Vec::new();
            let mut next_session = 1_u64;
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut request = [0_u8; 1024];
                        let size = stream.peek(&mut request).unwrap_or(0);
                        let request = String::from_utf8_lossy(&request[..size]);
                        if request.starts_with("GET /json/list")
                            || request.starts_with("GET /json ")
                        {
                            let mut consumed = Vec::new();
                            let mut chunk = [0_u8; 256];
                            loop {
                                let read = stream.read(&mut chunk).unwrap_or(0);
                                if read == 0 {
                                    break;
                                }
                                consumed.extend_from_slice(&chunk[..read]);
                                if consumed.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                                    break;
                                }
                            }
                            let body = json!([{
                                "id": TARGET_ID,
                                "type": "page",
                                "url": "app://-/index.html",
                                "webSocketDebuggerUrl": format!(
                                    "ws://127.0.0.1:{port}/devtools/page/{TARGET_ID}"
                                )
                            }]);
                            super::write_cdp_http(&mut stream, &body);
                        } else if request.starts_with("GET /devtools/page/") {
                            let session_id = next_session;
                            next_session += 1;
                            let (control_tx, control_rx) = mpsc::channel();
                            worker_state
                                .sessions
                                .lock()
                                .unwrap()
                                .insert(session_id, ObservedSession::default());
                            worker_state
                                .controls
                                .lock()
                                .unwrap()
                                .push(control_tx.clone());
                            let Ok(mut socket) = tungstenite::accept(stream) else {
                                continue;
                            };
                            let session_state = worker_state.clone();
                            let connections = connection_tx.clone();
                            let health_expression = health_expression.clone();
                            websocket_workers.push(thread::spawn(move || {
                                let _ = socket
                                    .get_mut()
                                    .set_read_timeout(Some(Duration::from_millis(40)));
                                let _ = connections.send(Connection {
                                    id: session_id,
                                    control: control_tx,
                                });
                                serve_session(
                                    &mut socket,
                                    session_id,
                                    &session_state,
                                    &control_rx,
                                    &health_expression,
                                );
                            }));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
            for worker in websocket_workers {
                let _ = worker.join();
            }
        });
        Self {
            port,
            stop,
            state,
            connections,
            server: Some(server),
        }
    }

    fn next_connection(&self, timeout: Duration) -> Connection {
        self.connections
            .recv_timeout(timeout)
            .expect("injection worker must open a page CDP session")
    }

    fn wait_for_method(&self, id: u64, method: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let observed = self
                .state
                .sessions
                .lock()
                .unwrap()
                .get(&id)
                .is_some_and(|session| session.methods.iter().any(|seen| seen == method));
            if observed {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_reload_replay(&self, id: u64, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let observed = self
                .state
                .sessions
                .lock()
                .unwrap()
                .get(&id)
                .is_some_and(|session| session.reload_replays > 0);
            if observed {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_expression(&self, needle: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let observed = self.state.sessions.lock().unwrap().values().any(|session| {
                session
                    .expressions
                    .iter()
                    .any(|expression| expression.contains(needle))
            });
            if observed {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_close(&self, id: u64, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let closed = self
                .state
                .sessions
                .lock()
                .unwrap()
                .get(&id)
                .is_some_and(|session| session.closed);
            if closed {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for ReloadFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for control in self.state.controls.lock().unwrap().iter() {
            let _ = control.send(Control::Disconnect);
        }
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }
}

struct InjectionWorker {
    alive: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for InjectionWorker {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

impl InjectionWorker {
    fn stop_and_join(&mut self) {
        self.alive.store(false, Ordering::Release);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}

fn serve_session(
    socket: &mut tungstenite::WebSocket<TcpStream>,
    session_id: u64,
    state: &FixtureState,
    controls: &mpsc::Receiver<Control>,
    health_expression: &str,
) {
    let mut script_registered = false;
    loop {
        match controls.try_recv() {
            Ok(Control::Reload) => {
                let replayed = script_registered;
                if replayed {
                    if let Some(session) = state.sessions.lock().unwrap().get_mut(&session_id) {
                        session.reload_replays += 1;
                    }
                }
                if socket
                    .send(Message::Text(
                        json!({"method": "Runtime.executionContextsCleared", "params": {}})
                            .to_string()
                            .into(),
                    ))
                    .is_err()
                {
                    break;
                }
            }
            Ok(Control::Disconnect) => {
                let _ = socket.close(None);
                break;
            }
            Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => {}
        }

        match socket.read() {
            Ok(Message::Text(text)) => {
                let Ok(command) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                let id = command.get("id").and_then(Value::as_u64).unwrap_or(0);
                let method = command
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let expression = command
                    .pointer("/params/expression")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if let Some(session) = state.sessions.lock().unwrap().get_mut(&session_id) {
                    session.expressions.push(expression.to_string());
                }
                if method == "Page.addScriptToEvaluateOnNewDocument" {
                    script_registered = true;
                }
                if let Some(session) = state.sessions.lock().unwrap().get_mut(&session_id) {
                    session.methods.push(method.to_string());
                }
                let result = if method == "Runtime.evaluate"
                    && expression.contains("officialBlockerVisible")
                {
                    if state.mode_available.load(Ordering::Acquire) {
                        json!({"result": {"value": {
                            "modeAvailable": true,
                            "modeLabel": "Codex",
                            "officialBlockerVisible": false
                        }}})
                    } else {
                        json!({"result": {"value": {
                            "modeAvailable": false,
                            "modeLabel": "",
                            "officialBlockerVisible": true
                        }}})
                    }
                } else if method == "Runtime.evaluate"
                    && expression.contains("__incodexRefreshProfileMaskHealth")
                {
                    json!({"result": {"value": true}})
                } else if method == "Runtime.evaluate" && expression == health_expression {
                    json!({"result": {"value": {
                        "button": true,
                        "banner": false,
                        "profileMask": true
                    }}})
                } else {
                    json!({"result": {}})
                };
                let response = json!({"id": id, "result": result});
                if socket
                    .send(Message::Text(response.to_string().into()))
                    .is_err()
                {
                    break;
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
    }
    if let Some(session) = state.sessions.lock().unwrap().get_mut(&session_id) {
        session.closed = true;
    }
}

fn wait_for_ready(status_rx: &mpsc::Receiver<InjectionStatus>, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return false;
        }
        match status_rx.recv_timeout(remaining) {
            Ok(InjectionStatus::Ready) => return true,
            Ok(InjectionStatus::BlockedByOfficialUi | InjectionStatus::ModeUnresolved(_)) => {}
            Ok(InjectionStatus::Failed(_) | InjectionStatus::RuntimeFailed(_)) => return false,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
}

#[test]
fn injection_worker_keeps_registration_for_reload_and_re_registers_same_target() {
    let fixture = ReloadFixture::start();
    let alive = Arc::new(AtomicBool::new(true));
    let (status_tx, status_rx) = mpsc::channel();
    let mut worker = InjectionWorker {
        alive: alive.clone(),
        thread: Some(start_injection_worker(
            fixture.port,
            InjectionOptions::default(),
            status_tx,
            Arc::new(AtomicBool::new(false)),
            alive,
        )),
    };

    assert!(wait_for_ready(&status_rx, Duration::from_secs(5)));
    let first = fixture.next_connection(Duration::from_secs(2));
    assert!(fixture.wait_for_method(
        first.id,
        "Page.addScriptToEvaluateOnNewDocument",
        Duration::from_secs(2)
    ));
    assert!(fixture.wait_for_method(first.id, "Runtime.evaluate", Duration::from_secs(2)));

    // 新文档脚本属于当前CDP会话。保持worker存活才能覆盖renderer reload。
    thread::sleep(Duration::from_millis(100));
    assert!(
        worker
            .thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished()),
        "injection worker exited after Ready and dropped the session-scoped registration"
    );
    assert!(first.control.send(Control::Reload).is_ok());
    assert!(fixture.wait_for_reload_replay(first.id, Duration::from_secs(2)));

    // 同一target断线后，新DevTools session必须重新注册新文档脚本。
    assert!(first.control.send(Control::Disconnect).is_ok());
    assert!(fixture.wait_for_close(first.id, Duration::from_secs(2)));
    let second = fixture.next_connection(Duration::from_secs(5));
    assert_eq!(
        second.id,
        first.id + 1,
        "fixture keeps the same page target"
    );
    assert!(fixture.wait_for_method(
        second.id,
        "Page.addScriptToEvaluateOnNewDocument",
        Duration::from_secs(3)
    ));

    worker.alive.store(false, Ordering::Release);
    worker.stop_and_join();
    assert!(fixture.wait_for_close(second.id, Duration::from_secs(3)));
    let duplicate_ready = status_rx
        .try_iter()
        .filter(|status| matches!(status, InjectionStatus::Ready))
        .count();
    assert_eq!(duplicate_ready, 0, "reload must not publish a second Ready");
}

#[test]
fn masked_worker_keeps_registration_session_while_health_monitor_runs() {
    let options = InjectionOptions {
        profile_mask: Some(ProfileMask {
            name: "Fixture Identity".into(),
            avatar: ProfileAvatar::Generated,
        }),
        ..InjectionOptions::default()
    };
    let fixture = ReloadFixture::start_with_options(options.clone());
    let alive = Arc::new(AtomicBool::new(true));
    let (status_tx, status_rx) = mpsc::channel();
    let mut worker = InjectionWorker {
        alive: alive.clone(),
        thread: Some(start_injection_worker(
            fixture.port,
            options,
            status_tx,
            Arc::new(AtomicBool::new(false)),
            alive,
        )),
    };

    assert!(wait_for_ready(&status_rx, Duration::from_secs(5)));
    let registration = fixture.next_connection(Duration::from_secs(2));
    assert!(fixture.wait_for_method(
        registration.id,
        "Page.addScriptToEvaluateOnNewDocument",
        Duration::from_secs(2)
    ));
    assert!(
        fixture.wait_for_expression("__incodexRefreshProfileMaskHealth", Duration::from_secs(3))
    );
    assert!(worker
        .thread
        .as_ref()
        .is_some_and(|thread| !thread.is_finished()));
    assert!(!fixture
        .state
        .sessions
        .lock()
        .unwrap()
        .get(&registration.id)
        .is_some_and(|session| session.closed));

    assert!(registration.control.send(Control::Reload).is_ok());
    assert!(fixture.wait_for_reload_replay(registration.id, Duration::from_secs(2)));

    worker.alive.store(false, Ordering::Release);
    worker.stop_and_join();
    assert!(fixture.wait_for_close(registration.id, Duration::from_secs(3)));
}

#[test]
fn blocked_short_sessions_do_not_cache_new_document_registration_for_ready_session() {
    let fixture = ReloadFixture::start();
    fixture.state.mode_available.store(false, Ordering::Release);
    let alive = Arc::new(AtomicBool::new(true));
    let (status_tx, status_rx) = mpsc::channel();
    let mut worker = InjectionWorker {
        alive: alive.clone(),
        thread: Some(start_injection_worker(
            fixture.port,
            InjectionOptions::default(),
            status_tx,
            Arc::new(AtomicBool::new(false)),
            alive,
        )),
    };

    let first = fixture.next_connection(Duration::from_secs(2));
    assert!(matches!(
        status_rx.recv_timeout(Duration::from_secs(2)),
        Ok(InjectionStatus::BlockedByOfficialUi)
    ));
    assert!(fixture.wait_for_close(first.id, Duration::from_secs(2)));
    let first_methods = fixture
        .state
        .sessions
        .lock()
        .unwrap()
        .get(&first.id)
        .unwrap()
        .methods
        .clone();
    assert!(!first_methods
        .iter()
        .any(|method| method == "Page.addScriptToEvaluateOnNewDocument"));
    assert!(!first_methods
        .iter()
        .any(|method| method == "Input.dispatchKeyEvent"));

    fixture.state.mode_available.store(true, Ordering::Release);
    let ready_session = fixture.next_connection(Duration::from_secs(3));
    assert!(wait_for_ready(&status_rx, Duration::from_secs(3)));
    assert!(fixture.wait_for_method(
        ready_session.id,
        "Page.addScriptToEvaluateOnNewDocument",
        Duration::from_secs(2)
    ));
    assert_eq!(ready_session.id, first.id + 1);

    worker.alive.store(false, Ordering::Release);
    worker.stop_and_join();
    assert!(fixture.wait_for_close(ready_session.id, Duration::from_secs(3)));
}
