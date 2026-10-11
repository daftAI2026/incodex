/*
 * [INPUT]: 依赖私有权限传输与无宿主测试夹具。
 * [OUTPUT]: 验证有界协议、语言选择与既有 AX 生命周期。
 * [POS]: 原 accessibility_guide_host 测试模块的独立文件。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

#[test]
fn accepts_only_nonce_bound_host_events() {
    assert_eq!(
        decode_host_event(br#"{"nonce":"abc","type":"ready"}"#, "abc"),
        Ok(HostEvent::Ready)
    );
    assert!(decode_host_event(br#"{"nonce":"wrong","type":"ready"}"#, "abc").is_err());
    assert!(decode_host_event(br#"{"nonce":"abc","type":"state"}"#, "abc").is_err());
}

#[test]
fn native_host_reports_the_target_app_locale_before_copy_is_selected() {
    assert_eq!(
        decode_host_event(
            br#"{"nonce":"abc","type":"app-locale","locale":"ar-SA"}"#,
            "abc",
        ),
        Ok(HostEvent::AppLocale("ar-SA".into()))
    );
    assert!(decode_host_event(
        br#"{"nonce":"wrong","type":"app-locale","locale":"ar-SA"}"#,
        "abc",
    )
    .is_err());
    assert!(decode_host_event(
        br#"{"nonce":"abc","type":"app-locale","locale":"../../wrong"}"#,
        "abc",
    )
    .is_err());
}

#[test]
fn rejects_oversized_transport_lines_without_parsing_them() {
    let (sender, receiver) = mpsc::sync_channel(2);
    let oversized = vec![b'x'; MAX_HOST_LINE_BYTES + 1];
    read_host_lines(std::io::Cursor::new(oversized), sender);
    assert!(matches!(receiver.recv().unwrap(), ReaderEvent::Oversized));
    assert!(receiver.try_recv().is_err());
}

#[test]
fn reader_emits_each_line_then_eof() {
    let (sender, receiver) = mpsc::sync_channel(4);
    read_host_lines(std::io::Cursor::new(b"first\nsecond\n"), sender);
    assert!(matches!(
        receiver.recv().unwrap(),
        ReaderEvent::Line(line) if line == b"first"
    ));
    assert!(matches!(
        receiver.recv().unwrap(),
        ReaderEvent::Line(line) if line == b"second"
    ));
    assert!(matches!(receiver.recv().unwrap(), ReaderEvent::Eof));
}

#[test]
fn reader_stops_cleanly_when_receiver_is_closed() {
    let (sender, receiver) = mpsc::sync_channel(1);
    drop(receiver);
    read_host_lines(std::io::Cursor::new(b"discarded\n"), sender);
}

#[test]
fn bounded_reader_channel_handles_output_flood_without_unbounded_queue() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader =
        thread::spawn(|| read_host_lines(std::io::Cursor::new(b"one\ntwo\nthree\n"), sender));
    for expected in [b"one".as_slice(), b"two", b"three"] {
        assert!(matches!(
            receiver.recv().unwrap(),
            ReaderEvent::Line(line) if line == expected
        ));
    }
    assert!(matches!(receiver.recv().unwrap(), ReaderEvent::Eof));
    reader.join().unwrap();
}

#[test]
fn nonzero_host_exit_after_stdout_eof_is_reported_as_error() {
    let mut child = Command::new("/bin/sh")
        .args(["-c", "exit 7"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let stdin = child.stdin.take();
    let (sender, events) = mpsc::sync_channel(8);
    thread::spawn(move || read_host_lines(stdout, sender));
    let mut host = ProcessGuideHost {
        child,
        stdin,
        events,
        nonce: "test".into(),
        closed: false,
    };
    assert!(matches!(
        host.poll(Duration::from_secs(1)).unwrap(),
        HostEvent::Error(message) if message.contains("exited unsuccessfully")
    ));
    host.close();
}

#[test]
fn native_copy_locale_resolution_matches_the_runtime_catalog_rules() {
    let catalog = [
        "en", "zh-CN", "zh-HK", "zh-TW", "de-DE", "es-419", "fr-FR", "nb-NO", "pt-BR", "ar", "fa",
        "ur",
    ]
    .into_iter()
    .map(|locale| (locale.to_string(), Value::Null))
    .collect();
    let cases = [
        ("", "en"),
        ("C.UTF-8", "en"),
        ("zh-MO", "zh-CN"),
        ("zh-Hant-HK", "zh-HK"),
        ("zh_Hant.UTF-8", "zh-TW"),
        ("de", "de-DE"),
        ("es", "es-419"),
        ("fr", "fr-FR"),
        ("no", "nb-NO"),
        ("pt", "pt-BR"),
        ("en-GB", "en"),
        ("unknown", "en"),
    ];
    for (raw, expected) in cases {
        assert_eq!(
            resolve_catalog_locale(raw, &catalog),
            expected,
            "locale {raw}"
        );
    }
}

#[test]
fn native_error_copy_points_to_explicit_reentry_not_read_only_doctor() {
    assert_eq!(
        reentry_error_body("Run incodex install to check again."),
        "Run incodex accessibility to check again."
    );
}

#[test]
fn native_guide_copy_follows_verified_target_identity() {
    let mut installed = serde_json::json!({
        "title": "Enable ChatGPT script control",
        "installedTitle": "Re-enable ChatGPT script control",
        "body": "Installing Incodex changes ChatGPT.",
        "officialBody": "This is the official ChatGPT app.",
    });
    let original = installed.clone();
    choose_guide_copy(
        installed.as_object_mut().unwrap(),
        GuideCopyContext::Installed,
    )
    .unwrap();
    assert_eq!(installed["title"], "Re-enable ChatGPT script control");
    assert_eq!(installed["body"], "Installing Incodex changes ChatGPT.");

    let mut official = original;
    choose_guide_copy(
        official.as_object_mut().unwrap(),
        GuideCopyContext::Official,
    )
    .unwrap();
    assert_eq!(official["title"], "Enable ChatGPT script control");
    assert_eq!(official["body"], "This is the official ChatGPT app.");

    let mut missing = serde_json::json!({ "body": "Install only" });
    assert!(
        choose_guide_copy(missing.as_object_mut().unwrap(), GuideCopyContext::Official).is_err()
    );
    let mut missing_installed_title = serde_json::json!({
        "title": "Enable ChatGPT script control",
        "body": "Install only",
    });
    assert!(choose_guide_copy(
        missing_installed_title.as_object_mut().unwrap(),
        GuideCopyContext::Installed,
    )
    .is_err());
}

#[test]
fn embedded_native_copy_has_both_reasons_for_all_65_locales() {
    let source = incodex_runtime_assets::external_files()
        .iter()
        .find_map(|(name, body)| (*name == PERMISSION_COPY_NAME).then_some(*body))
        .expect("embedded permission catalog");
    let catalog: serde_json::Map<String, Value> = serde_json::from_str(source).unwrap();
    assert_eq!(catalog.len(), 65);
    for (locale, entry) in catalog {
        let installed_title = entry["installedTitle"]
            .as_str()
            .expect("localized installed title");
        let title = entry["title"].as_str().expect("localized official title");
        let install = entry["body"].as_str().expect("localized install reason");
        let official = entry["officialBody"]
            .as_str()
            .expect("localized official reason");
        let instruction = entry["addedTitle"].as_str().expect("instruction");
        assert!(!install.trim().is_empty(), "{locale}");
        assert!(!installed_title.trim().is_empty(), "{locale}");
        assert!(!title.trim().is_empty(), "{locale}");
        assert_ne!(
            installed_title, title,
            "{locale} needs a localized re-enable title"
        );
        assert!(!official.trim().is_empty(), "{locale}");
        assert!(official.contains("ChatGPT"), "{locale}");
        assert_ne!(install, official, "{locale}");
        assert_ne!(install, instruction, "{locale}");

        let mut installed = entry.as_object().unwrap().clone();
        choose_guide_copy(&mut installed, GuideCopyContext::Installed).unwrap();
        assert_eq!(installed["title"], installed_title, "{locale}");
        assert_eq!(installed["body"], install, "{locale}");
        let mut restored = entry.as_object().unwrap().clone();
        choose_guide_copy(&mut restored, GuideCopyContext::Official).unwrap();
        assert_eq!(restored["title"], title, "{locale}");
        assert_eq!(restored["body"], official, "{locale}");
    }
}

#[test]
fn native_copy_layout_direction_follows_the_canonical_rtl_locale() {
    let catalog: serde_json::Map<String, Value> = [
        ("en".into(), Value::Null),
        ("ar".into(), Value::Null),
        ("fa".into(), Value::Null),
        ("ur".into(), Value::Null),
        ("zh-CN".into(), Value::Null),
        ("zh-HK".into(), Value::Null),
        ("de-DE".into(), Value::Null),
    ]
    .into_iter()
    .collect();
    for raw in ["ar", "AR-SA", "fa_IR", "ur-PK"] {
        assert!(is_rtl_locale(&resolve_catalog_locale(raw, &catalog)));
    }
    for raw in ["", "zh-Hant-HK", "de-DE", "unknown"] {
        assert!(!is_rtl_locale(&resolve_catalog_locale(raw, &catalog)));
    }
}

#[test]
fn open_settings_command_uses_background_open() {
    assert_eq!(
            ACCESSIBILITY_SETTINGS_URL,
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility"
        );
    let command = open_settings_command(ACCESSIBILITY_SETTINGS_URL);
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(command.get_program().to_string_lossy(), "/usr/bin/open");
    assert_eq!(
        args,
        vec!["-g".to_string(), ACCESSIBILITY_SETTINGS_URL.to_string()]
    );
}

#[test]
fn configured_locale_prefers_a_nonempty_config_override() {
    assert_eq!(select_locale_source(Some("fr-FR"), Some("ar-SA")), "fr-FR");
}

#[test]
fn configured_locale_falls_back_to_the_target_app_before_english() {
    assert_eq!(select_locale_source(Some("  "), Some("ar-SA")), "ar-SA");
    assert_eq!(select_locale_source(None, Some("zh-Hant-HK")), "zh-Hant-HK");
    assert_eq!(select_locale_source(None, None), "en");
}

#[test]
fn locale_source_home_matches_the_runtime_source_home_override() {
    let fallback = Path::new("/tmp/default-codex-home");
    assert_eq!(
        resolve_locale_source_home(
            Some(std::ffi::OsStr::new("/tmp/selected-codex-home")),
            fallback
        ),
        PathBuf::from("/tmp/selected-codex-home")
    );
    assert_eq!(resolve_locale_source_home(None, fallback), fallback);
}

struct FormalProcessHostFactory {
    executable: PathBuf,
    pid_file: PathBuf,
}

impl GuideHostFactory for FormalProcessHostFactory {
    fn start(&mut self, _: &Path, _: &Path) -> Result<Box<dyn GuideHost>, String> {
        let host = ProcessGuideHost::spawn_for_test(&self.executable, GuideCopyContext::Installed)?;
        fs::write(&self.pid_file, host.child.id().to_string())
            .map_err(|error| format!("cannot publish G10 child-host PID: {error}"))?;
        Ok(Box::new(host))
    }
}

#[derive(Default)]
struct DiagnosticFailureOps {
    reset_calls: usize,
    settings_calls: usize,
}

impl GuideOps for DiagnosticFailureOps {
    fn probe_screen(&mut self) -> Option<ScreenPermissionStatus> {
        None
    }
    fn launch(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn probe(&mut self) -> AccessibilityStatus {
        AccessibilityStatus::Denied
    }

    fn wait_for_window(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn reset(&mut self) -> Result<(), String> {
        self.reset_calls += 1;
        Err("G10_DIAGNOSTIC_RESET_FAILURE".into())
    }

    fn open_settings(&mut self) -> Result<(), String> {
        self.settings_calls += 1;
        Err("G10 diagnostic must not open System Settings".into())
    }

    fn wait(&mut self, _: Duration) {}
}

#[derive(Default)]
struct DiagnosticOpenSettingsFailureOps {
    reset_calls: usize,
    settings_calls: usize,
}

impl GuideOps for DiagnosticOpenSettingsFailureOps {
    fn probe_screen(&mut self) -> Option<ScreenPermissionStatus> {
        None
    }
    fn launch(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn probe(&mut self) -> AccessibilityStatus {
        AccessibilityStatus::Denied
    }

    fn wait_for_window(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn reset(&mut self) -> Result<(), String> {
        self.reset_calls += 1;
        Ok(())
    }

    fn open_settings(&mut self) -> Result<(), String> {
        self.settings_calls += 1;
        Err("G10_DIAGNOSTIC_OPEN_SETTINGS_FAILURE".into())
    }

    fn wait(&mut self, _: Duration) {}
}

#[derive(Default)]
struct ScriptedHostLog {
    states: Vec<HostState>,
    closed: bool,
}

struct ScriptedGuideHost {
    events: VecDeque<HostEvent>,
    log: Rc<RefCell<ScriptedHostLog>>,
}

impl GuideHost for ScriptedGuideHost {
    fn send_state(&mut self, state: HostState) -> Result<(), String> {
        self.log.borrow_mut().states.push(state);
        Ok(())
    }

    fn poll(&mut self, _: Duration) -> Result<HostEvent, String> {
        Ok(self.events.pop_front().unwrap_or(HostEvent::Timeout))
    }

    fn close(&mut self) {
        self.log.borrow_mut().closed = true;
    }
}

struct ScriptedGuideHostFactory {
    events: VecDeque<HostEvent>,
    log: Rc<RefCell<ScriptedHostLog>>,
}

impl GuideHostFactory for ScriptedGuideHostFactory {
    fn start(&mut self, _: &Path, _: &Path) -> Result<Box<dyn GuideHost>, String> {
        Ok(Box::new(ScriptedGuideHost {
            events: std::mem::take(&mut self.events),
            log: Rc::clone(&self.log),
        }))
    }
}

#[test]
fn mocked_open_settings_failure_sends_terminal_error_and_returns_original_error() {
    let mut ops = DiagnosticOpenSettingsFailureOps::default();
    let log = Rc::new(RefCell::new(ScriptedHostLog::default()));
    let mut factory = ScriptedGuideHostFactory {
        events: [HostEvent::Ready, HostEvent::Allow, HostEvent::Later].into(),
        log: Rc::clone(&log),
    };
    let mut verify = || Ok(());

    let result = run_permission_guide_with_timeouts(
        &mut ops,
        Path::new("/private/tmp/incodex-g10-diagnostic-root"),
        Path::new("/Applications/ChatGPT.app"),
        &mut verify,
        &mut factory,
        Duration::from_secs(1),
        Duration::from_secs(1),
    );

    assert_eq!(result, Err("G10_DIAGNOSTIC_OPEN_SETTINGS_FAILURE".into()));
    assert_eq!(ops.reset_calls, 1, "only the no-op reset fixture may run");
    assert_eq!(
        ops.settings_calls, 1,
        "only the injected Settings failure may run"
    );
    let log = log.borrow();
    assert_eq!(
        log.states,
        vec![
            HostState::Repairing,
            HostState::Error("G10_DIAGNOSTIC_OPEN_SETTINGS_FAILURE".into()),
        ],
    );
    assert!(log.closed, "Later must close the test host");
}

#[test]
#[ignore = "opt-in visible UI diagnostic; run only through permission-cli-host-error.test.ts"]
fn rust_coordinator_process_host_displays_mocked_reset_failure_until_dismissed() {
    let executable = env::var_os("INCODEX_G10_HOST_EXECUTABLE")
        .map(PathBuf::from)
        .expect("G10 test host executable path");
    let pid_file = env::var_os("INCODEX_G10_HOST_PID_FILE")
        .map(PathBuf::from)
        .expect("G10 test host PID file path");
    let test_pid_file = env::var_os("INCODEX_G10_TEST_PID_FILE")
        .map(PathBuf::from)
        .expect("G10 Rust test PID file path");
    fs::write(test_pid_file, std::process::id().to_string()).expect("publish G10 Rust test PID");
    let mut ops = DiagnosticFailureOps::default();
    let mut factory = FormalProcessHostFactory {
        executable,
        pid_file,
    };
    let mut verify = || Ok(());

    let result = run_permission_guide_with_timeouts(
        &mut ops,
        Path::new("/private/tmp/incodex-g10-diagnostic-root"),
        Path::new("/Applications/ChatGPT.app"),
        &mut verify,
        &mut factory,
        Duration::from_secs(30),
        Duration::from_secs(1),
    );

    assert_eq!(result, Err("G10_DIAGNOSTIC_RESET_FAILURE".into()));
    assert_eq!(ops.reset_calls, 1, "only the fake reset may be attempted");
    assert_eq!(
        ops.settings_calls, 0,
        "the fake flow must never open Settings"
    );
}

#[test]
#[ignore = "opt-in visible UI diagnostic; run only through permission-cli-host-error.test.ts"]
fn rust_coordinator_process_host_displays_mocked_open_settings_failure_until_dismissed() {
    let executable = env::var_os("INCODEX_G10_HOST_EXECUTABLE")
        .map(PathBuf::from)
        .expect("G10 test host executable path");
    let pid_file = env::var_os("INCODEX_G10_HOST_PID_FILE")
        .map(PathBuf::from)
        .expect("G10 host PID file path");
    let test_pid_file = env::var_os("INCODEX_G10_TEST_PID_FILE")
        .map(PathBuf::from)
        .expect("G10 Rust test PID file path");
    fs::write(test_pid_file, std::process::id().to_string()).expect("publish G10 Rust test PID");
    let mut ops = DiagnosticOpenSettingsFailureOps::default();
    let mut factory = FormalProcessHostFactory {
        executable,
        pid_file,
    };
    let mut verify = || Ok(());

    let result = run_permission_guide_with_timeouts(
        &mut ops,
        Path::new("/private/tmp/incodex-g10-diagnostic-root"),
        Path::new("/Applications/ChatGPT.app"),
        &mut verify,
        &mut factory,
        Duration::from_secs(30),
        Duration::from_secs(1),
    );

    assert_eq!(result, Err("G10_DIAGNOSTIC_OPEN_SETTINGS_FAILURE".into()));
    assert_eq!(ops.reset_calls, 1, "only the no-op reset fixture may run");
    assert_eq!(
        ops.settings_calls, 1,
        "only the injected Settings failure may run"
    );
    println!(
        "G10_OPEN_SETTINGS_FAILURE result={result:?} resetCalls={} settingsCalls={}",
        ops.reset_calls, ops.settings_calls
    );
}
