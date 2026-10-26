/*
 * [INPUT]: 依赖真实安装事务、dist Loader、nonce 生产器和 Swift stdio 入口。
 * [OUTPUT]: 验证默认 install 收尾、短命探针、原引导终态和最后启动顺序。
 * [POS]: 共用边界串联回归；TCC 与冷启动是替身；window-close 注入配置延迟。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;
use crate::permission_screen_probe::InstalledScreenProbe;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::OnceLock;

#[path = "permission_install_chain_fixture.rs"]
mod fixture;
use fixture::Fixture;

impl Fixture {
    fn marker(&self) -> PathBuf {
        self.root
            .join("transactions")
            .join(&self.install_id)
            .join("accessibility-setup.json")
    }
}

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn private_directory(path: &Path) {
    fs::create_dir_all(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn private_json(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn json_lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn wait_for_receipt(path: &Path, event: &str, child: &mut Child) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = json_lines(path)
            .into_iter()
            .find(|value| value["event"] == event)
        {
            return value;
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "Runtime child exited: {:?}",
            json_lines(path)
        );
        assert!(
            Instant::now() < deadline,
            "Runtime receipt timed out: {:?}",
            json_lines(path)
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn native_fixture() -> &'static Path {
    static HOST: OnceLock<PathBuf> = OnceLock::new();
    HOST.get_or_init(|| {
        let directory = env::temp_dir().join(format!(
            "incodex-install-chain-native-{}",
            std::process::id()
        ));
        private_directory(&directory);
        let executable = directory.join("permission-host");
        let root = repository();
        let output = Command::new("/usr/bin/xcrun")
            .args([
                "swiftc",
                "-D",
                "INCODEX_PERMISSION_HOST_TESTING",
                "-module-name",
                "InstallPermissionChain",
            ])
            .arg(root.join("native/macos/permission-host.swift"))
            .arg(root.join("crates/incodex-cli/tests/native/install-permission-presenter.swift"))
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Swift fixture: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        executable
    })
}

fn delayed_native_fixture() -> &'static Path {
    static HOST: OnceLock<PathBuf> = OnceLock::new();
    HOST.get_or_init(|| {
        let directory = env::temp_dir().join(format!(
            "incodex-install-chain-delayed-native-{}",
            std::process::id()
        ));
        private_directory(&directory);
        let executable = directory.join("permission-host");
        let root = repository();
        let presenter = root.join("crates/incodex-cli/tests/native/install-permission-presenter.swift");
        let source = fs::read_to_string(&presenter).unwrap();
        let original = "func configurePermissions(_ statuses: [String: String]) {}";
        assert_eq!(source.matches(original).count(), 1, "合成 Presenter 延迟插入点必须唯一");
        let delayed = source.replacen(
            original,
            "func configurePermissions(_ statuses: [String: String]) { Thread.sleep(forTimeInterval: 0.2) }",
            1,
        );
        let delayed_presenter = directory.join("install-permission-presenter.swift");
        fs::write(&delayed_presenter, delayed).unwrap();
        let output = Command::new("/usr/bin/xcrun")
            .args([
                "swiftc",
                "-D",
                "INCODEX_PERMISSION_HOST_TESTING",
                "-module-name",
                "InstallPermissionChainDelayed",
            ])
            .arg(root.join("native/macos/permission-host.swift"))
            .arg(&delayed_presenter)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "delayed Swift fixture: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        executable
    })
}

// ---- 同一真实进程身份；只替换 OS 的 App 路径解析和权限读数 ----
struct RuntimeProbe {
    child: Child,
    screen: Option<InstalledScreenProbe>,
    ax: AccessibilityStatus,
    receipt: PathBuf,
    retired: bool,
}

impl install::ProbeSession for RuntimeProbe {
    fn exited(&mut self) -> Result<bool, String> {
        self.child
            .try_wait()
            .map(|status| status.is_some())
            .map_err(|error| error.to_string())
    }
    fn snapshot(&mut self) -> PermissionSnapshot {
        let pid = self.child.id() as i32;
        let ax = self.ax;
        system::coherent_snapshot(
            || {
                (
                    incodex_macos::AccessibilityReport {
                        status: ax,
                        pid: Some(pid),
                        reason: None,
                    },
                    incodex_core::session::process_start_identity(pid),
                )
            },
            || {
                Some(
                    self.screen
                        .as_mut()
                        .unwrap()
                        .probe_with_pid(Path::new(OFFICIAL_APP_PATH), || {
                            incodex_core::session::process_start_identity(pid).map(|_| pid)
                        }),
                )
            },
        )
    }
    fn observed(&self) -> bool {
        self.screen
            .as_ref()
            .is_some_and(|screen| screen.has_observation())
    }
    fn retire(&mut self) {
        let directory = self.receipt.parent().unwrap().join(".incodex/transactions");
        for transaction in fs::read_dir(directory).unwrap().filter_map(Result::ok) {
            for name in [
                "permission-probe-request.json",
                "permission-probe-response.json",
            ] {
                let source = transaction.path().join(name);
                if source.is_file() {
                    fs::copy(source, self.receipt.parent().unwrap().join(name)).unwrap();
                }
            }
        }
        drop(self.screen.take());
        self.retired = true;
    }
    fn finish(&mut self) -> Result<(), String> {
        assert!(self.retired, "先退休 nonce，再等待 probe 退出");
        install::wait_for_launcher(&mut self.child)?;
        assert!(self.child.try_wait().unwrap().is_some());
        let rows = json_lines(&self.receipt);
        let quit = rows
            .iter()
            .find(|row| row["event"] == "probe-quit")
            .expect("real Runtime quit");
        assert_eq!(quit["requestRemovalObserved"], true);
        assert!(!rows.iter().any(|row| row["event"] == "normal-open"));
        Ok(())
    }
}

impl Drop for RuntimeProbe {
    fn drop(&mut self) {
        drop(self.screen.take());
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = install::wait_for_launcher(&mut self.child);
        }
    }
}

fn runtime_child(
    home: &Path,
    id: &str,
    request: &str,
    status: &str,
    mode: &str,
) -> (Child, PathBuf) {
    private_directory(home);
    let receipt = home.join("lifecycle.jsonl");
    let config = home.join("config.json");
    private_json(
        &config,
        &json!({
            "home": home, "installId": id, "requestId": request,
            "screenStatus": status, "mode": mode, "receipt": receipt,
            "distDirectory": repository().join("dist"),
        }),
    );
    let child = Command::new("bun")
        .arg(repository().join("crates/incodex-cli/tests/fixtures/install-permission-runtime.cjs"))
        .arg(&config)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    (child, receipt)
}

struct ChainOps<'a> {
    fixture: &'a Fixture,
    cache: install::ProbeCache,
    launches: usize,
    initial: &'static str,
    ax: AccessibilityStatus,
    action: &'static str,
    ready: Rc<Cell<bool>>,
    polls: usize,
    traces: Rc<RefCell<Vec<GuideTraceEvent>>>,
    children: Vec<(i32, String)>,
}

impl ChainOps<'_> {
    fn cold(&mut self, granted: bool) -> PermissionSnapshot {
        let marker: Value =
            serde_json::from_slice(&fs::read(self.fixture.marker()).unwrap()).unwrap();
        let request = marker["requestId"].as_str().unwrap().to_owned();
        let home = self
            .fixture
            .sandbox
            .join(format!("probe-{}", self.launches));
        let transaction = home
            .join(".incodex/transactions")
            .join(&self.fixture.install_id);
        for path in [
            home.join(".incodex"),
            home.join(".incodex/transactions"),
            transaction.clone(),
        ] {
            private_directory(&path);
        }
        // 这是测试 OS 命名空间映射。真实 CAS 的身份不变；仅映射默认 App 路径。
        let mut wire_marker = marker;
        wire_marker["appPath"] = json!(OFFICIAL_APP_PATH);
        private_json(&transaction.join("accessibility-setup.json"), &wire_marker);
        let (mut child, receipt) = runtime_child(
            &home,
            &self.fixture.install_id,
            &request,
            if granted { "granted" } else { self.initial },
            "probe",
        );
        let ready = wait_for_receipt(&receipt, "probe-ready", &mut child);
        let pid = child.id() as i32;
        let start = incodex_core::session::process_start_identity(pid).unwrap();
        assert_eq!(ready["pid"], pid);
        assert_eq!(ready["processStartIdentity"], start);
        self.children.push((pid, start));
        self.launches += 1;
        let screen = InstalledScreenProbe::for_request_for_test(
            &home.join(".incodex"),
            Path::new(OFFICIAL_APP_PATH),
            &self.fixture.install_id,
            &request,
        )
        .unwrap();
        let mut session = RuntimeProbe {
            child,
            screen: Some(screen),
            ax: if granted {
                AccessibilityStatus::Granted
            } else {
                self.ax
            },
            receipt,
            retired: false,
        };
        let result = install::collect_snapshot(&mut session).unwrap();
        let request_value: Value =
            serde_json::from_slice(&fs::read(home.join("permission-probe-request.json")).unwrap())
                .unwrap();
        let response_value: Value =
            serde_json::from_slice(&fs::read(home.join("permission-probe-response.json")).unwrap())
                .unwrap();
        assert_eq!(response_value["nonce"], request_value["nonce"]);
        assert_eq!(response_value["pid"], pid);
        assert!(
            response_value["observedAtMs"].as_u64().unwrap()
                - request_value["requestedAtMs"].as_u64().unwrap()
                <= 600
        );
        assert!(!transaction.join("permission-probe-request.json").exists());
        assert!(!transaction.join("permission-probe-response.json").exists());
        result
    }
}

impl GuideOps for ChainOps<'_> {
    fn launch(&mut self) -> Result<(), String> {
        panic!("不能提前打开工作宿主")
    }
    fn probe(&mut self) -> AccessibilityStatus {
        self.snapshot().accessibility
    }
    fn snapshot(&mut self) -> PermissionSnapshot {
        let ready = self.ready.get();
        if ready {
            self.polls += 1;
        }
        let changed = ready && self.action == "grant" && self.polls == 3;
        let mut cache = std::mem::take(&mut self.cache);
        let result = cache.read(changed, || self.cold(changed));
        self.cache = cache;
        if ready && self.polls < 3 {
            assert_eq!(self.launches, 1, "闲置卡不得重复冷启动");
        }
        result
    }
    fn observe(&mut self, event: GuideTraceEvent) {
        self.traces.borrow_mut().push(event);
    }
    fn wait_for_window(&mut self) -> Result<(), String> {
        panic!("安装模式不等工作窗")
    }
    fn reset(&mut self) -> Result<(), String> {
        panic!("隔离不得触碰 TCC")
    }
    fn reset_screen(&mut self) -> Result<(), String> {
        panic!("隔离不得触碰 TCC")
    }
    fn open_settings(&mut self) -> Result<(), String> {
        panic!("隔离不得打开系统设置")
    }
    fn wait(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
}

struct ChainHost {
    host: ProcessGuideHost,
    control: PathBuf,
    ready: Rc<Cell<bool>>,
    action: &'static str,
    states: Rc<RefCell<Vec<HostState>>>,
    eof_pending: bool,
}
impl GuideHost for ChainHost {
    fn send_state(&mut self, state: HostState) -> Result<(), String> {
        self.states.borrow_mut().push(state.clone());
        self.host.send_state(state)
    }
    fn send_permissions(&mut self, snapshot: &PermissionSnapshot) -> Result<(), String> {
        self.host.send_permissions(snapshot)
    }
    fn send_permission_state(&mut self, permission: &str, state: HostState) -> Result<(), String> {
        self.host.send_permission_state(permission, state)
    }
    fn poll(&mut self, timeout: Duration) -> Result<HostEvent, String> {
        if self.eof_pending {
            drop(self.host.stdin.take());
            self.eof_pending = false;
        }
        let event = self.host.poll(timeout)?;
        if event == HostEvent::Ready {
            self.ready.set(true);
            match self.action {
                "skip" | "window-close" => fs::write(&self.control, self.action).unwrap(),
                // Ready 后先让 flow 完成初始 permissions 写入，再制造真实 EOF。
                "eof" => self.eof_pending = true,
                _ => {}
            }
        }
        Ok(event)
    }
    fn close(&mut self) {
        self.host.close();
    }
}

struct ChainFactory {
    executable: PathBuf,
    control: PathBuf,
    action: &'static str,
    ready: Rc<Cell<bool>>,
    states: Rc<RefCell<Vec<HostState>>>,
    starts: usize,
}
impl GuideHostFactory for ChainFactory {
    fn start(&mut self, _: &Path, _: &Path) -> Result<Box<dyn GuideHost>, String> {
        panic!("必须传双行权限")
    }
    fn start_with_permissions(
        &mut self,
        _: &Path,
        _: &Path,
        snapshot: &PermissionSnapshot,
    ) -> Result<Box<dyn GuideHost>, String> {
        self.starts += 1;
        Ok(Box::new(ChainHost {
            host: ProcessGuideHost::spawn_executable_with_permissions(
                &self.executable,
                GuideCopyContext::Installing,
                Some(snapshot),
            )?,
            control: self.control.clone(),
            ready: self.ready.clone(),
            action: self.action,
            states: self.states.clone(),
            eof_pending: false,
        }))
    }
}

#[test]
fn install_chain_default_dispatch_runtime_nonce_native_terminal_then_normal_open() {
    let parsed = crate::parse::parse_cli(&["install".into(), "--yes".into()]).unwrap();
    assert!(parsed.live && parsed.app.is_none());
    for (label, status, action, expected, cards, probes, fail_final) in [
        ("grant", "denied", "grant", "granted", 1, 2, false),
        ("dual-grant", "denied", "grant", "granted", 1, 2, false),
        ("already-granted", "granted", "none", "granted", 0, 1, false),
        ("unknown", "unknown", "none", "pending", 0, 1, false),
        ("skip", "denied", "skip", "deferred", 1, 1, false),
        (
            "window-close",
            "denied",
            "window-close",
            "deferred",
            1,
            1,
            false,
        ),
        ("eof", "denied", "eof", "pending", 1, 1, false),
        (
            "changed-final-target",
            "granted",
            "none",
            "granted",
            0,
            1,
            true,
        ),
    ] {
        let fixture = Fixture::new(label);
        let binary = fixture.sandbox.join("native-host.bin");
        let host_fixture = if label == "window-close" {
            delayed_native_fixture()
        } else {
            native_fixture()
        };
        fs::copy(host_fixture, &binary).unwrap();
        let wrapper = fixture.sandbox.join("native-host");
        let locale_app = fixture.sandbox.join("locale.app");
        fs::create_dir_all(locale_app.join("Contents")).unwrap();
        fs::write(locale_app.join("Contents/Info.plist"), r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.openai.codex</string><key>CFBundleDevelopmentRegion</key><string>en</string><key>CFBundleLocalizations</key><array><string>en</string></array></dict></plist>"#).unwrap();
        fs::write(
            &wrapper,
            format!(
                "#!/bin/sh\nexport INCODEX_PERMISSION_HOST_TARGET_BUNDLE='{}'\nexec '{}' \"$@\"\n",
                locale_app.display(),
                binary.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700)).unwrap();
        let ready = Rc::new(Cell::new(false));
        let traces = Rc::new(RefCell::new(Vec::new()));
        let states = Rc::new(RefCell::new(Vec::new()));
        let mut ops = ChainOps {
            fixture: &fixture,
            cache: install::ProbeCache::default(),
            launches: 0,
            initial: status,
            ax: if label == "dual-grant" {
                AccessibilityStatus::Denied
            } else {
                AccessibilityStatus::Granted
            },
            action,
            ready: ready.clone(),
            polls: 0,
            traces: traces.clone(),
            children: vec![],
        };
        let mut factory = ChainFactory {
            executable: wrapper,
            control: PathBuf::from(format!("{}.control", binary.display())),
            action,
            ready,
            states: states.clone(),
            starts: 0,
        };
        let opens = Cell::new(0);
        let dispatch = crate::install::install_permission_setup::run_after_install(
            &parsed,
            &fixture.root,
            &fixture.app,
            Some(&fixture.install_id),
            |before, verify| {
                assert!(before, "真实当前 Loader 选择前置入口");
                let started = Instant::now();
                let result = flow::run_permission_guide_with_mode(
                    &mut ops,
                    &fixture.root,
                    &fixture.app,
                    &mut || verify(),
                    &mut factory,
                    GuideTimeouts {
                        ready: Duration::from_secs(5),
                        choice: Some(Duration::from_secs(10)),
                        guide: None,
                    },
                    flow::GuideMode::BeforeLaunch,
                );
                if let Err(error) = &result {
                    let mut safe_error = error.clone();
                    for path in [
                        fixture.sandbox.as_path(),
                        native_fixture().parent().unwrap(),
                    ] {
                        let path = path.to_string_lossy();
                        safe_error = safe_error.replace(path.as_ref(), "<fixture>");
                    }
                    let safe_error = safe_error.chars().take(512).collect::<String>();
                    eprintln!(
                        "INSTALL_CHAIN_GUIDE_ERROR {}",
                        json!({
                            "scenario": label,
                            "expected": expected,
                            "elapsedMs": started.elapsed().as_millis(),
                            "nativeHostStarts": factory.starts,
                            "probeLaunches": ops.launches,
                            "probePolls": ops.polls,
                            "error": safe_error,
                        })
                    );
                }
                result
            },
            |verify| {
                if fail_final {
                    fs::write(fixture.app.join("Contents/Resources/changed"), b"changed").unwrap();
                }
                install::launch_after_install_with(
                    verify,
                    || {
                        assert!(ops_children_gone(&fixture.sandbox));
                        Ok(())
                    },
                    || {
                        let marker: Value =
                            serde_json::from_slice(&fs::read(fixture.marker()).unwrap()).unwrap();
                        assert_eq!(marker["state"], expected, "终态先于正常启动");
                        let home = fixture.sandbox.join("normal-home");
                        let (mut child, receipt) = runtime_child(
                            &home,
                            &fixture.install_id,
                            marker["requestId"].as_str().unwrap(),
                            status,
                            "normal",
                        );
                        install::wait_for_launcher(&mut child)?;
                        let events = json_lines(&receipt);
                        assert_eq!(
                            events
                                .iter()
                                .filter(|row| row["event"] == "normal-open")
                                .count(),
                            1
                        );
                        assert!(!events.iter().any(|row| row["event"] == "probe-ready"));
                        opens.set(opens.get() + 1);
                        Ok(())
                    },
                )
            },
        )
        .unwrap();
        assert_eq!(
            dispatch,
            crate::install::install_permission_setup::InstallPermissionDispatch::Completed
        );
        assert_eq!(factory.starts, cards, "{label}");
        assert_eq!(ops.launches, probes, "{label}");
        assert_eq!(opens.get(), usize::from(!fail_final), "{label}");
        let marker: Value = serde_json::from_slice(&fs::read(fixture.marker()).unwrap()).unwrap();
        assert_eq!(marker["state"], expected, "{label}");
        if action == "grant" {
            assert!(states.borrow().contains(&HostState::Granted));
        }
        if action == "eof" {
            assert!(traces.borrow().contains(&GuideTraceEvent::HostEof));
        }
        if action == "skip" || action == "window-close" {
            assert!(traces.borrow().contains(&GuideTraceEvent::UserSkip));
        }
        eprintln!(
            "INSTALL_CHAIN_PASS {}",
            json!({"scenario": label, "markerState": expected,
            "nativeCards": factory.starts, "coldProbes": ops.launches, "normalOpens": opens.get(),
            "children": ops.children, "traces": traces.borrow().iter().map(|event| event.log_event()).collect::<Vec<_>>() })
        );
        if let Some(directory) = env::var_os("INCODEX_INSTALL_CHAIN_EVIDENCE") {
            let destination = PathBuf::from(directory).join(label);
            private_directory(&destination);
            fs::copy(
                fixture.marker(),
                destination.join("accessibility-setup.json"),
            )
            .unwrap();
            for entry in fs::read_dir(&fixture.sandbox)
                .unwrap()
                .filter_map(Result::ok)
            {
                let path = entry.path().join("lifecycle.jsonl");
                if path.is_file() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    fs::copy(path, destination.join(format!("{name}.jsonl"))).unwrap();
                    for envelope in [
                        "permission-probe-request.json",
                        "permission-probe-response.json",
                    ] {
                        let source = entry.path().join(envelope);
                        if source.is_file() {
                            fs::copy(source, destination.join(format!("{name}-{envelope}")))
                                .unwrap();
                        }
                    }
                }
            }
        }
    }
}

fn ops_children_gone(sandbox: &Path) -> bool {
    fs::read_dir(sandbox)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("probe-"))
        .all(|entry| {
            let rows = json_lines(&entry.path().join("lifecycle.jsonl"));
            let Some(subject) = rows.iter().find(|row| row["event"] == "child-start") else {
                return false;
            };
            let pid = subject["pid"].as_i64().unwrap() as i32;
            let start = subject["processStartIdentity"].as_str().unwrap();
            rows.iter().any(|row| row["event"] == "process-exit")
                && incodex_core::session::process_start_identity(pid).as_deref() != Some(start)
        })
}

#[test]
fn install_chain_custom_target_does_not_claim_default_preflight() {
    let parsed =
        crate::parse::parse_cli(&["install".into(), "--app".into(), "/test/ChatGPT.app".into()])
            .unwrap();
    let result = crate::install::install_permission_setup::run_after_install(
        &parsed,
        Path::new("/unused"),
        Path::new("/unused/ChatGPT.app"),
        None,
        |_, _| panic!("--app 不得冒充默认 install 链"),
        |_| panic!("--app 不得自动打开"),
    )
    .unwrap();
    assert_eq!(
        result,
        crate::install::install_permission_setup::InstallPermissionDispatch::NotSelected
    );
}

#[test]
fn install_chain_unrecorded_terminal_never_dispatches_normal_launch() {
    let fixture = Fixture::new("terminal-proof-failed");
    let parsed = crate::parse::parse_cli(&["install".into(), "--yes".into()]).unwrap();
    let launches = Cell::new(0);
    crate::install::install_permission_setup::run_after_install(
        &parsed,
        &fixture.root,
        &fixture.app,
        Some(&fixture.install_id),
        |before, verify| {
            assert!(before);
            verify()?;
            fs::write(fixture.app.join("Contents/Resources/changed"), b"changed").unwrap();
            Ok(Outcome::Granted)
        },
        |_| {
            launches.set(launches.get() + 1);
            Ok(())
        },
    )
    .unwrap();
    let marker: Value = serde_json::from_slice(&fs::read(fixture.marker()).unwrap()).unwrap();
    assert_eq!(
        marker["state"], "awaiting-user",
        "失效安装证明不能写入用户终态"
    );
    assert_eq!(launches.get(), 0, "终态未保存，不能派发正常启动");
}
