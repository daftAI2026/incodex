//! [INPUT]: 依赖真实进程owner handoff、只读窗口观察器与loopback CDP。
//! [OUTPUT]: 验证spawn责任归属与阻挡期间的主窗几何探测。
//! [POS]: open生命周期回归；只启动合成child，不启动官方App或调用授权API。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use super::super::process::{spawn_child, ChildProcess as _};
use super::super::{
    burn_session_home, prepare_incognito_open, spawn_plan_with_owner_and_native_close,
    BurnExpected, OpenProcessResult,
};
use super::{fake_app, temp_root};
use serde_json::Value;
use std::fs;
use std::fs::OpenOptions;
use std::io::Read;
use std::net::{Ipv4Addr, TcpListener};
use std::os::fd::AsRawFd;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const REPORT_ENV: &str = "INCODEX_TEST_OPEN_RESPONSIBILITY_REPORT";
const ADAPTER_REPORT_ENV: &str = "INCODEX_TEST_ADAPTER_RESPONSIBILITY_REPORT";
const CONTRACT_REPORT_ENV: &str = "INCODEX_TEST_OPEN_PROCESS_REPORT";
const CONTRACT_OVERRIDE_ENV: &str = "INCODEX_TEST_OPEN_PROCESS_OVERRIDE";
const CONTRACT_FD_ENV: &str = "INCODEX_TEST_OPEN_PROCESS_FD";
const BLOCKED_BOUNDS_CHILD_ENV: &str = "INCODEX_TEST_BLOCKED_BOUNDS_CHILD";

struct RemoveFixture(PathBuf);

impl Drop for RemoveFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct BlockedCdpFixture {
    port: u16,
    stop: Arc<AtomicBool>,
    mode_probes: Arc<AtomicUsize>,
    health_probes: Arc<AtomicUsize>,
    bounds_probes: Arc<AtomicUsize>,
    worker: Option<JoinHandle<()>>,
}

impl BlockedCdpFixture {
    fn start() -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let mode_probes = Arc::new(AtomicUsize::new(0));
        let health_probes = Arc::new(AtomicUsize::new(0));
        let bounds_probes = Arc::new(AtomicUsize::new(0));
        let worker_stop = stop.clone();
        let worker_mode_probes = mode_probes.clone();
        let worker_health_probes = health_probes.clone();
        let worker_bounds_probes = bounds_probes.clone();
        let worker = thread::spawn(move || {
            let mut websocket_workers = Vec::new();
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
                        let mut peek = [0_u8; 1024];
                        let request_deadline = Instant::now() + Duration::from_secs(2);
                        let size = loop {
                            match stream.peek(&mut peek) {
                                Ok(size)
                                    if size > 0
                                        && peek[..size]
                                            .windows(4)
                                            .any(|bytes| bytes == b"\r\n\r\n") =>
                                {
                                    break size;
                                }
                                Ok(_) => {}
                                Err(error)
                                    if matches!(
                                        error.kind(),
                                        std::io::ErrorKind::WouldBlock
                                            | std::io::ErrorKind::TimedOut
                                    ) =>
                                {
                                    if Instant::now() >= request_deadline {
                                        break 0;
                                    }
                                }
                                Err(_) => break 0,
                            }
                            if worker_stop.load(Ordering::Acquire)
                                || Instant::now() >= request_deadline
                            {
                                break 0;
                            }
                            thread::sleep(Duration::from_millis(1));
                        };
                        if size == 0 {
                            continue;
                        }
                        let request = String::from_utf8_lossy(&peek[..size]);
                        if request.starts_with("GET /devtools/") {
                            let stop = worker_stop.clone();
                            let mode_probes = worker_mode_probes.clone();
                            let health_probes = worker_health_probes.clone();
                            let bounds_probes = worker_bounds_probes.clone();
                            websocket_workers.push(thread::spawn(move || {
                                let Ok(mut socket) = tungstenite::accept(stream) else {
                                    return;
                                };
                                let _ = socket
                                    .get_mut()
                                    .set_read_timeout(Some(Duration::from_millis(100)));
                                while !stop.load(Ordering::Acquire) {
                                    let message = match socket.read() {
                                        Ok(message) => message,
                                        Err(tungstenite::Error::Io(error))
                                            if matches!(
                                                error.kind(),
                                                std::io::ErrorKind::WouldBlock
                                                    | std::io::ErrorKind::TimedOut
                                            ) =>
                                        {
                                            continue;
                                        }
                                        Err(_) => return,
                                    };
                                    let tungstenite::Message::Text(text) = message else {
                                        continue;
                                    };
                                    let Ok(command) = serde_json::from_str::<Value>(&text) else {
                                        return;
                                    };
                                    let Some(id) = command.get("id").and_then(Value::as_u64) else {
                                        continue;
                                    };
                                    let method = command.get("method").and_then(Value::as_str);
                                    let expression = command
                                        .pointer("/params/expression")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default();
                                    let is_eval = method == Some("Runtime.evaluate");
                                    let response = if is_eval
                                        && expression.contains("officialBlockerVisible")
                                    {
                                        mode_probes.fetch_add(1, Ordering::AcqRel);
                                        serde_json::json!({
                                            "id": id,
                                            "result": {"result": {"value": {
                                                "modeAvailable": false,
                                                "modeLabel": "",
                                                "officialBlockerVisible": true
                                            }}}
                                        })
                                    } else if is_eval
                                        && expression.contains("bounds.every(Number.isSafeInteger)")
                                    {
                                        bounds_probes.fetch_add(1, Ordering::AcqRel);
                                        serde_json::json!({
                                            "id": id,
                                            "result": {"result": {"value": [120, 100, 1280, 800]}}
                                        })
                                    } else if is_eval
                                        && expression == crate::cdp::ui_ready_expression()
                                    {
                                        health_probes.fetch_add(1, Ordering::AcqRel);
                                        serde_json::json!({
                                            "id": id,
                                            "result": {"result": {"value": {
                                                "button": false,
                                                "banner": false
                                            }}}
                                        })
                                    } else if is_eval
                                        && expression
                                            .contains("__incodexUiProbe?.button === 'present'")
                                    {
                                        serde_json::json!({
                                            "id": id,
                                            "result": {"result": {"value": true}}
                                        })
                                    } else {
                                        serde_json::json!({"id": id, "result": {}})
                                    };
                                    if socket
                                        .send(tungstenite::Message::Text(
                                            response.to_string().into(),
                                        ))
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                            }));
                        } else {
                            let mut request = Vec::new();
                            let mut chunk = [0_u8; 1024];
                            loop {
                                match stream.read(&mut chunk) {
                                    Ok(0) => break,
                                    Ok(size) => {
                                        request.extend_from_slice(&chunk[..size]);
                                        if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                                            break;
                                        }
                                    }
                                    Err(error)
                                        if matches!(
                                            error.kind(),
                                            std::io::ErrorKind::WouldBlock
                                                | std::io::ErrorKind::TimedOut
                                        ) =>
                                    {
                                        break;
                                    }
                                    Err(_) => break,
                                }
                            }
                            let body = serde_json::json!([{
                                "id": "blocked-main",
                                "type": "page",
                                "url": "app://-/index.html",
                                "webSocketDebuggerUrl": format!(
                                    "ws://127.0.0.1:{port}/devtools/page/blocked-main"
                                )
                            }]);
                            super::write_cdp_http(&mut stream, &body);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => return,
                }
            }
            for worker in websocket_workers {
                let _ = worker.join();
            }
        });
        Self {
            port,
            stop,
            mode_probes,
            health_probes,
            bounds_probes,
            worker: Some(worker),
        }
    }
}

impl Drop for BlockedCdpFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn responsible_pid(pid: libc::pid_t) -> libc::pid_t {
    type Getter = unsafe extern "C" fn(libc::pid_t) -> libc::pid_t;
    let symbol = unsafe {
        libc::dlsym(
            libc::RTLD_DEFAULT,
            c"responsibility_get_pid_responsible_for_pid".as_ptr(),
        )
    };
    assert!(
        !symbol.is_null(),
        "responsibility getter is required for this test"
    );
    let getter: Getter = unsafe { std::mem::transmute(symbol) };
    unsafe { getter(pid) }
}

#[test]
fn private_open_child_reports_process_responsibility() {
    if let Some(path) = std::env::var_os(ADAPTER_REPORT_ENV) {
        let report = serde_json::json!({
            "pid": std::process::id(),
            "ppid": unsafe { libc::getppid() },
            "responsible_pid": responsible_pid(std::process::id() as libc::pid_t),
            "executable": std::env::current_exe().unwrap(),
        });
        fs::write(path, serde_json::to_vec(&report).unwrap()).unwrap();
        return;
    }

    let root = temp_root();
    let _remove_fixture = RemoveFixture(root.clone());
    let report_path = root.join("responsibility.json");
    let parent_pid = std::process::id();
    let parent_responsible_pid = responsible_pid(parent_pid as libc::pid_t);
    let executable = std::env::current_exe().unwrap();
    let mut child = spawn_child(
        &executable,
        &[
            "--exact",
            "open::open_tests::open_process_tests::private_open_child_reports_process_responsibility",
            "--nocapture",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>(),
        &[(
            ADAPTER_REPORT_ENV.to_string(),
            report_path.display().to_string(),
        )],
    )
    .unwrap();
    let child_pid = child.id();
    let status = child.wait().unwrap();
    assert!(status.success(), "isolated child test must exit normally");

    let report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    assert_eq!(report["pid"].as_u64(), Some(u64::from(child_pid)));
    assert_eq!(report["ppid"].as_i64(), Some(i64::from(parent_pid)));
    assert_eq!(
        report["responsible_pid"].as_i64(),
        report["pid"].as_i64(),
        "private-open child must own responsibility instead of inheriting its CLI parent; parent responsibility is {parent_responsible_pid}"
    );
    assert_eq!(
        report["executable"].as_str(),
        Some(executable.to_str().unwrap())
    );
}

fn wait_for_owner_handoff() {
    let session_root = std::env::var_os("INCODEX_SESSION_ROOT")
        .expect("open plan must pass its isolated session root");
    let owner_path = PathBuf::from(session_root).join("owner.json");
    let expected_pid = std::process::id() as i64;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(bytes) = fs::read(&owner_path) {
            if serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|owner| owner.get("pid").and_then(Value::as_i64))
                == Some(expected_pid)
            {
                return;
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("open production path did not hand session ownership to child");
}

fn stdio_is_null_device() -> bool {
    let mut null_stat = unsafe { std::mem::zeroed::<libc::stat>() };
    if unsafe { libc::stat(c"/dev/null".as_ptr(), &mut null_stat) } != 0 {
        return false;
    }
    (libc::STDIN_FILENO..=libc::STDERR_FILENO).all(|fd| {
        let mut descriptor_stat = unsafe { std::mem::zeroed::<libc::stat>() };
        (unsafe { libc::fstat(fd, &mut descriptor_stat) }) == 0
            && descriptor_stat.st_rdev == null_stat.st_rdev
            && descriptor_stat.st_mode & libc::S_IFMT == libc::S_IFCHR
    })
}

fn inherited_file_matches_fd_spec() -> bool {
    let Some(spec) = std::env::var_os(CONTRACT_FD_ENV) else {
        return false;
    };
    file_descriptor_matches_spec(&spec.to_string_lossy())
}

fn file_descriptor_matches_spec(spec: &str) -> bool {
    let fields: Vec<_> = spec.split(':').collect();
    let [fd, device, inode] = fields.as_slice() else {
        return false;
    };
    let (Ok(fd), Ok(device), Ok(inode)) = (
        (*fd).parse::<libc::c_int>(),
        (*device).parse::<u64>(),
        (*inode).parse::<u64>(),
    ) else {
        return false;
    };
    let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
    (unsafe { libc::fstat(fd, &mut stat) }) == 0
        && stat.st_dev as u64 == device
        && stat.st_ino as u64 == inode
}

#[test]
fn private_open_production_child_owns_process_responsibility() {
    if let Some(report_path) = std::env::var_os(REPORT_ENV) {
        wait_for_owner_handoff();
        let report = serde_json::json!({
            "pid": std::process::id(),
            "ppid": unsafe { libc::getppid() },
            "responsible_pid": responsible_pid(std::process::id() as libc::pid_t),
            "executable": std::env::current_exe().unwrap(),
        });
        fs::write(report_path, serde_json::to_vec(&report).unwrap()).unwrap();
        return;
    }

    let root = temp_root();
    let _remove_fixture = RemoveFixture(root.clone());
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let parent_pid = std::process::id();
    let parent_responsible_pid = responsible_pid(parent_pid as libc::pid_t);
    let executable = std::env::current_exe().unwrap();
    let report_path = root.join("responsibility.json");
    let mut plan = prepare_incognito_open(&app, &user, &source, parent_pid as i32).unwrap();
    plan.bin = executable.clone();
    plan.args = vec![
        "--exact".into(),
        "open::open_tests::open_process_tests::private_open_production_child_owns_process_responsibility".into(),
        "--nocapture".into(),
    ];
    plan.env
        .push((REPORT_ENV.into(), report_path.display().to_string()));
    plan.debug_port = 0;
    plan.profile_mask = None;

    let outcome = spawn_plan_with_owner_and_native_close(&plan, None).unwrap();
    assert!(matches!(
        outcome.process,
        OpenProcessResult::Exited {
            code: 0,
            ui_ready: false
        }
    ));
    let report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();
    burn_session_home(
        &plan.session_root,
        &BurnExpected {
            user_root: &user,
            session_id: Some(&plan.session_id),
            ino: Some(plan.session_ino),
            dev: Some(plan.session_dev),
        },
    )
    .unwrap();

    assert!(report["pid"].as_i64().is_some_and(|pid| pid > 0));
    assert_ne!(report["pid"].as_i64(), Some(i64::from(parent_pid)));
    assert_eq!(report["ppid"].as_i64(), Some(i64::from(parent_pid)));
    assert_eq!(
        report["responsible_pid"].as_i64(),
        report["pid"].as_i64(),
        "production open child inherited responsibility from parent PID {parent_responsible_pid}"
    );
    assert_eq!(
        report["executable"].as_str(),
        Some(executable.to_str().unwrap())
    );
}

#[test]
fn spawn_fixture_reports_environment_stdio_and_inherited_fd_contract() {
    if let Some(report_path) = std::env::var_os(CONTRACT_REPORT_ENV) {
        let report = serde_json::json!({
            "pid": std::process::id(),
            "ppid": unsafe { libc::getppid() },
            "path": std::env::var("PATH").unwrap(),
            "override": std::env::var(CONTRACT_OVERRIDE_ENV).unwrap(),
            "stdio_null": stdio_is_null_device(),
            "inherited_fd_matches": inherited_file_matches_fd_spec(),
        });
        fs::write(report_path, serde_json::to_vec(&report).unwrap()).unwrap();
        return;
    }

    let root = temp_root();
    let _remove_fixture = RemoveFixture(root.clone());
    let report_path = root.join("process-contract.json");
    let inherited_path = root.join("inherited-descriptor");
    fs::write(&inherited_path, "do not inherit\n").unwrap();
    let inherited_file = OpenOptions::new().read(true).open(&inherited_path).unwrap();
    let fd = inherited_file.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    assert!(flags >= 0);
    assert_eq!(
        unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) },
        0,
        "fixture must expose a non-CLOEXEC file descriptor"
    );
    let mut inherited_stat = unsafe { std::mem::zeroed::<libc::stat>() };
    assert_eq!(unsafe { libc::fstat(fd, &mut inherited_stat) }, 0);
    let fd_spec = format!(
        "{}:{}:{}",
        fd, inherited_stat.st_dev as u64, inherited_stat.st_ino as u64
    );
    assert!(
        file_descriptor_matches_spec(&fd_spec),
        "same-process control must identify the open fixture descriptor"
    );
    let executable = std::env::current_exe().unwrap();
    let args = [
        "--exact",
        "open::open_tests::open_process_tests::spawn_fixture_reports_environment_stdio_and_inherited_fd_contract",
        "--nocapture",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    let env = [
        (
            CONTRACT_REPORT_ENV.to_string(),
            report_path.display().to_string(),
        ),
        (
            CONTRACT_OVERRIDE_ENV.to_string(),
            "from-open-plan".to_string(),
        ),
        (CONTRACT_FD_ENV.to_string(), fd_spec),
    ];
    let mut child = spawn_child(&executable, &args, &env).unwrap();
    let child_pid = child.id();
    let status = child.wait().unwrap();
    assert!(
        status.success(),
        "process contract fixture must exit normally"
    );
    let report: Value = serde_json::from_slice(&fs::read(&report_path).unwrap()).unwrap();

    assert_eq!(report["pid"].as_u64(), Some(u64::from(child_pid)));
    assert_eq!(report["ppid"].as_i64(), Some(i64::from(std::process::id())));
    assert_eq!(
        report["path"].as_str(),
        std::env::var("PATH").ok().as_deref()
    );
    assert_eq!(report["override"].as_str(), Some("from-open-plan"));
    assert_eq!(report["stdio_null"].as_bool(), Some(true));
    assert_eq!(report["inherited_fd_matches"].as_bool(), Some(false));
}

#[test]
fn spawn_preserves_arguments_with_spaces() {
    let root = temp_root();
    let _remove_fixture = RemoveFixture(root.clone());
    let report_path = root.join("arguments.txt");
    let script = r#"printf '%s\n%s\n%s\n' "$0" "$1" "$2" > "$INCODEX_TEST_ARG_REPORT""#;
    let args = vec![
        "-c".into(),
        script.into(),
        "argv zero with spaces".into(),
        "first argument with spaces".into(),
        "second argument with spaces".into(),
    ];
    let env = [(
        "INCODEX_TEST_ARG_REPORT".into(),
        report_path.display().to_string(),
    )];
    let mut child = spawn_child(&PathBuf::from("/bin/sh"), &args, &env).unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(0));
    assert_eq!(
        fs::read_to_string(report_path).unwrap(),
        "argv zero with spaces\nfirst argument with spaces\nsecond argument with spaces\n"
    );
}

#[test]
fn spawn_rejects_invalid_inputs_and_does_not_fabricate_a_child() {
    assert!(spawn_child(&PathBuf::from("/no/such/incodex-executable"), &[], &[]).is_err());
    assert!(spawn_child(&PathBuf::from("/bin/\0sh"), &[], &[]).is_err());
    assert!(spawn_child(&PathBuf::from("/bin/sh"), &["bad\0arg".into()], &[]).is_err());
    assert!(spawn_child(
        &PathBuf::from("/bin/sh"),
        &[],
        &[("INCODEX_TEST_NUL".into(), "bad\0value".into())]
    )
    .is_err());
    assert!(spawn_child(
        &PathBuf::from("/bin/sh"),
        &[],
        &[("INVALID=KEY".into(), "value".into())]
    )
    .is_err());
}

#[test]
fn spawn_accepts_relative_executable_path_with_slash() {
    let mut relative_shell = PathBuf::new();
    for component in std::env::current_dir().unwrap().components() {
        if matches!(component, std::path::Component::Normal(_)) {
            relative_shell.push("..");
        }
    }
    relative_shell.push("bin/sh");

    let mut child = spawn_child(&relative_shell, &["-c".into(), "exit 7".into()], &[]).unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(7));
}

#[test]
fn waitpid_preserves_exit_codes_and_caches_reaped_status() {
    for expected_code in [0, 7] {
        let args = vec!["-c".into(), format!("exit {expected_code}")];
        let mut child = spawn_child(&PathBuf::from("/bin/sh"), &args, &[]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "real child did not exit in time");
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(status.code(), Some(expected_code));
        assert_eq!(child.wait().unwrap(), status);
        assert_eq!(child.try_wait().unwrap(), Some(status));
        assert!(child.kill().is_err(), "a reaped PID must not be signaled");
    }
}

#[test]
fn waitpid_preserves_real_term_and_kill_signals() {
    for signal in [libc::SIGTERM, libc::SIGKILL] {
        let mut child = spawn_child(&PathBuf::from("/bin/sleep"), &["30".into()], &[]).unwrap();
        child
            .signal(signal)
            .unwrap_or_else(|error| panic!("signal {signal}: {error}"));
        let status = child.wait().unwrap();
        assert_eq!(status.signal(), Some(signal));
    }
}

#[test]
fn spawn_restores_sigpipe_default_for_official_child() {
    let mut child = spawn_child(
        &PathBuf::from("/bin/sh"),
        &["-c".into(), "kill -PIPE $$".into()],
        &[],
    )
    .unwrap();
    let status = child.wait().unwrap();
    assert_eq!(status.signal(), Some(libc::SIGPIPE));
}

#[test]
fn primary_window_bounds_are_probed_while_official_ui_stays_blocked() {
    if std::env::var_os(BLOCKED_BOUNDS_CHILD_ENV).is_some() {
        wait_for_owner_handoff();
        thread::sleep(Duration::from_millis(2_000));
        return;
    }

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = PathBuf::from("/private/tmp").join(format!(
        "incodex-blocked-bounds-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let _remove_fixture = RemoveFixture(root.clone());
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();

    let mut plan = prepare_incognito_open(&app, &user, &source, std::process::id() as i32).unwrap();
    plan.bin = std::env::current_exe().unwrap();
    plan.args = vec![
        "--exact".into(),
        "open::open_tests::open_process_tests::primary_window_bounds_are_probed_while_official_ui_stays_blocked"
            .into(),
        "--nocapture".into(),
    ];
    plan.env.push((BLOCKED_BOUNDS_CHILD_ENV.into(), "1".into()));
    plan.profile_mask = None;

    let cdp = BlockedCdpFixture::start();
    plan.debug_port = cdp.port;
    let outcome = spawn_plan_with_owner_and_native_close(
        &plan,
        Some(
            super::super::NativeCloseMonitor::new()
                .expect("read-only WindowServer observer must be available"),
        ),
    )
    .unwrap();
    burn_session_home(
        &plan.session_root,
        &BurnExpected {
            user_root: &user,
            session_id: Some(&plan.session_id),
            ino: Some(plan.session_ino),
            dev: Some(plan.session_dev),
        },
    )
    .unwrap();

    assert!(matches!(
        outcome.process,
        OpenProcessResult::Exited {
            code: 0,
            ui_ready: false
        }
    ));
    assert!(cdp.mode_probes.load(Ordering::Acquire) > 0);
    assert_eq!(
        cdp.health_probes.load(Ordering::Acquire),
        0,
        "the fixture must keep MODE Blocked throughout the owned child lifecycle"
    );
    assert!(
        cdp.bounds_probes.load(Ordering::Acquire) > 0,
        "the production supervisor must query its unique primary page bounds before MODE Ready"
    );
}
