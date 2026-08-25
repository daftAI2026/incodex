// Tests for the native `open` lifecycle. Kept outside open.rs so the product
// implementation remains below the repository's per-file size budget.
use super::*;
use incodex_core::session::create_session_home;
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::Message;

pub(super) fn temp_root() -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("incodex-open-unit-{n}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

pub(super) fn fake_app(root: &Path) -> PathBuf {
    let app = root.join("ChatGPT.app");
    let mac = app.join("Contents/MacOS");
    fs::create_dir_all(&mac).unwrap();
    fs::write(
        app.join("Contents/Info.plist"),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>ChatGPT</string></dict></plist>\n",
    )
    .unwrap();
    let executable = mac.join("ChatGPT");
    fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
    }
    fs::set_permissions(executable, permissions).unwrap();
    app
}

fn write_cdp_http(stream: &mut TcpStream, value: &serde_json::Value) {
    let body = value.to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).unwrap();
}

fn serve_early_close_cdp(
    listener: TcpListener,
    marker: PathBuf,
    ui_probed: Arc<AtomicBool>,
    browser_closed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    hide_primary_on_first_list: bool,
) {
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut list_requests = 0_u32;
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                let mut peek = [0_u8; 2048];
                let size = stream.peek(&mut peek).unwrap();
                let request = String::from_utf8_lossy(&peek[..size]);
                if request.starts_with("GET /devtools/") {
                    let browser = request.starts_with("GET /devtools/browser/");
                    let probed = ui_probed.clone();
                    let closed = browser_closed.clone();
                    let marker = marker.clone();
                    thread::spawn(move || {
                        let mut socket = tungstenite::accept(stream).unwrap();
                        while let Ok(Message::Text(text)) = socket.read() {
                            let command: serde_json::Value = serde_json::from_str(&text).unwrap();
                            let id = command
                                .get("id")
                                .and_then(serde_json::Value::as_u64)
                                .unwrap();
                            let method = command.get("method").and_then(serde_json::Value::as_str);
                            let health = method == Some("Runtime.evaluate")
                                && command
                                    .pointer("/params/expression")
                                    .and_then(serde_json::Value::as_str)
                                    == Some(crate::cdp::ui_ready_expression());
                            let response = if health {
                                probed.store(true, Ordering::Release);
                                serde_json::json!({
                                    "id": id,
                                    "result": {"result": {"value": {"button": true, "banner": false}}}
                                })
                            } else {
                                serde_json::json!({"id": id, "result": {}})
                            };
                            socket
                                .send(Message::Text(response.to_string().into()))
                                .unwrap();
                            if browser && method == Some("Browser.close") {
                                closed.store(true, Ordering::Release);
                                fs::write(&marker, "closed\n").unwrap();
                                break;
                            }
                            if health {
                                break;
                            }
                        }
                    });
                } else {
                    let mut raw = [0_u8; 2048];
                    let size = stream.read(&mut raw).unwrap();
                    let request = String::from_utf8_lossy(&raw[..size]);
                    if request.starts_with("GET /json/version ") {
                        write_cdp_http(
                            &mut stream,
                            &serde_json::json!({
                                "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/browser/test")
                            }),
                        );
                    } else {
                        list_requests += 1;
                        let targets = if hide_primary_on_first_list && list_requests == 1 {
                            serde_json::json!([])
                        } else if ui_probed.load(Ordering::Acquire) {
                            serde_json::json!([{
                                "id": "overlay",
                                "type": "page",
                                "url": "app://-/index.html?initialRoute=%2Favatar-overlay",
                                "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/overlay")
                            }])
                        } else {
                            serde_json::json!([{
                                "id": "main",
                                "type": "page",
                                "url": "app://-/index.html",
                                "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/main")
                            }])
                        };
                        write_cdp_http(&mut stream, &targets);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("CDP test server failed: {error}"),
        }
    }
}

#[test]
fn copy_failure_burns_the_session() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    let bin = resolve_executable(&app).unwrap();
    let target_id = target_id_from_exec(&bin.to_string_lossy());
    let session = create_session_home(&user, Some(&target_id), 1, "").unwrap();
    fs::remove_dir_all(&session.home).unwrap();
    assert!(copy_settings(&session.home, &source).is_err());
    burn_session_home(
        &session.root,
        &BurnExpected {
            user_root: &user,
            session_id: Some(&session.session_id),
            ino: Some(session.ino),
            dev: Some(session.dev),
        },
    )
    .unwrap();
    assert!(!session.root.exists());
}

#[test]
fn open_preparation_seeds_live_bounds_instead_of_stale_disk_bounds() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join(".codex-global-state.json"),
        r#"{"electron-main-window-bounds":{"x":0,"y":38,"width":1710,"height":1073,"isMaximized":true}}"#,
    )
    .unwrap();

    let plan = prepare_incognito_open_with_geometry(
        &app,
        &user,
        &source,
        1,
        Some(incodex_core::session::WindowGeometry {
            x: 597,
            y: 34,
            width: 869,
            height: 1073,
        }),
    )
    .unwrap();
    let state: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(plan.home.join(".codex-global-state.json")).unwrap(),
    )
    .unwrap();

    assert_eq!(
        state.get("electron-main-window-bounds"),
        Some(&serde_json::json!({
            "x": 597,
            "y": 34,
            "width": 869,
            "height": 1073
        }))
    );
}

#[test]
fn open_progress_distinguishes_launch_ready_and_waiting() {
    let (opening, opened, waiting) = open_progress_copy();
    assert_eq!(opening, "Opening incognito Codex window");
    assert_eq!(opened, "Opened. Incognito Codex window is ready.");
    assert_eq!(waiting, "Waiting for the window to close");
}

#[test]
fn closing_the_primary_window_before_ui_ready_still_stops_the_isolated_process() {
    let root = temp_root();
    let app = fake_app(&root);
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let marker = source.join("browser-closed");
    let executable = app.join("Contents/MacOS/ChatGPT");
    fs::write(
        &executable,
        "#!/bin/sh\nwhile [ ! -f \"$INCODEX_SOURCE_HOME/browser-closed\" ]; do sleep 0.02; done\n",
    )
    .unwrap();

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let ui_probed = Arc::new(AtomicBool::new(false));
    let browser_closed = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let server = {
        let marker = marker.clone();
        let ui_probed = ui_probed.clone();
        let browser_closed = browser_closed.clone();
        let stop = stop.clone();
        thread::spawn(move || {
            serve_early_close_cdp(listener, marker, ui_probed, browser_closed, stop, false)
        })
    };

    let user = root.join("home");
    let mut plan = prepare_incognito_open(&app, &user, &source, std::process::id() as i32).unwrap();
    plan.debug_port = port;
    let (done_tx, done_rx) = mpsc::channel();
    thread::spawn(move || {
        done_tx.send(spawn_plan(&plan)).unwrap();
    });

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && !browser_closed.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(20));
    }
    if !browser_closed.load(Ordering::Acquire) {
        fs::write(&marker, "test cleanup\n").unwrap();
    }
    let result = done_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    stop.store(true, Ordering::Release);
    server.join().unwrap();

    assert!(
        browser_closed.load(Ordering::Acquire),
        "the lifecycle monitor must start when the primary target is discovered, not after UI health"
    );
    assert!(matches!(result, OpenProcessResult::Exited { .. }));
}

#[test]
fn primary_discovered_during_failed_injection_still_gets_a_lifecycle_monitor() {
    let root = temp_root();
    let app = fake_app(&root);
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let marker = source.join("browser-closed");
    let executable = app.join("Contents/MacOS/ChatGPT");
    fs::write(
        &executable,
        "#!/bin/sh\nwhile [ ! -f \"$INCODEX_SOURCE_HOME/browser-closed\" ]; do sleep 0.02; done\n",
    )
    .unwrap();

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let ui_probed = Arc::new(AtomicBool::new(false));
    let browser_closed = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let server = {
        let marker = marker.clone();
        let ui_probed = ui_probed.clone();
        let browser_closed = browser_closed.clone();
        let stop = stop.clone();
        thread::spawn(move || {
            serve_early_close_cdp(listener, marker, ui_probed, browser_closed, stop, true)
        })
    };

    let user = root.join("home");
    let mut plan = prepare_incognito_open(&app, &user, &source, std::process::id() as i32).unwrap();
    plan.debug_port = port;
    let (done_tx, done_rx) = mpsc::channel();
    thread::spawn(move || {
        done_tx.send(spawn_plan(&plan)).unwrap();
    });

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && !browser_closed.load(Ordering::Acquire) {
        thread::sleep(Duration::from_millis(20));
    }
    if !browser_closed.load(Ordering::Acquire) {
        fs::write(&marker, "test cleanup\n").unwrap();
    }
    let result = done_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    stop.store(true, Ordering::Release);
    server.join().unwrap();

    assert!(
        ui_probed.load(Ordering::Acquire),
        "the primary must appear during injection before this regression is exercised"
    );
    assert!(
        browser_closed.load(Ordering::Acquire),
        "a primary first discovered inside a failed injection attempt must still be monitored"
    );
    assert!(matches!(result, OpenProcessResult::Exited { .. }));
}

#[test]
fn ready_published_between_status_poll_and_child_exit_is_not_lost() {
    let (status_tx, status_rx) = mpsc::channel();
    let readiness = AtomicBool::new(false);

    // spawn_plan polls status_rx before child.try_wait. The first poll is
    // empty; the producer then publishes Ready while the child exits.
    assert!(matches!(
        status_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    publish_injection_status(&status_tx, &readiness, InjectionStatus::Ready);

    // No second channel poll happens before this child-exit observation.
    // The producer's acceptance must already be visible here.
    assert!(
        readiness.load(Ordering::Acquire),
        "Ready published between lifecycle polls must survive child exit"
    );
}

#[test]
fn profile_mask_failure_after_ready_revokes_ui_acceptance() {
    let (status_tx, _status_rx) = mpsc::channel();
    let readiness = AtomicBool::new(false);

    publish_injection_status(&status_tx, &readiness, InjectionStatus::Ready);
    assert!(readiness.load(Ordering::Acquire));
    publish_injection_status(
        &status_tx,
        &readiness,
        InjectionStatus::Failed("profile mask health failed".into()),
    );

    assert!(
        !readiness.load(Ordering::Acquire),
        "a post-start mask failure must revoke the accepted UI state"
    );
}

#[test]
fn lifecycle_exit_codes_distinguish_process_ui_and_cleanup_failures() {
    let removed = CleanupResult::Removed { attempts: 1 };
    let retained = CleanupResult::Retained {
        attempts: 5,
        retained_path: PathBuf::from("/tmp/session"),
        reason: "EPERM".into(),
    };
    assert_eq!(
        OpenProcessResult::Exited {
            code: 0,
            ui_ready: true,
        }
        .exit_code(&removed),
        OpenExitCode::Success
    );
    assert_eq!(
        OpenProcessResult::SpawnFailed {
            error: "ENOENT".into()
        }
        .exit_code(&removed),
        OpenExitCode::ProcessFailure
    );
    assert_eq!(
        OpenProcessResult::Exited {
            code: 7,
            ui_ready: true,
        }
        .exit_code(&removed),
        OpenExitCode::ProcessFailure
    );
    assert_eq!(
        OpenProcessResult::Exited {
            code: 0,
            ui_ready: false,
        }
        .exit_code(&removed),
        OpenExitCode::UiInjectionFailure
    );
    assert_eq!(
        OpenProcessResult::Exited {
            code: 0,
            ui_ready: true,
        }
        .exit_code(&retained),
        OpenExitCode::CleanupRetained
    );
}

#[test]
fn burn_failure_does_not_claim_removed() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
    let (process, cleanup) = wait_and_burn_with(
        &plan,
        &user,
        0,
        |_| {
            Ok(OpenProcessResult::Exited {
                code: 0,
                ui_ready: true,
            })
        },
        |_, _| Err("EPERM".into()),
    )
    .unwrap();
    assert_eq!(
        process.exit_code(&cleanup),
        OpenExitCode::CleanupRetained,
        "retained session must have a distinct lifecycle code"
    );
    assert!(plan.session_root.exists());
    assert_eq!(
        cleanup,
        CleanupResult::Retained {
            attempts: 5,
            retained_path: plan.session_root.clone(),
            reason: "EPERM".into(),
        }
    );
    let (ok, message) = format_session_cleanup(&cleanup);
    assert!(!ok);
    assert!(!message.to_lowercase().contains("removed"));
}

#[test]
fn wait_and_burn_passes_the_created_session_identity_to_cleanup() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
    let owner: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(plan.session_root.join("owner.json")).unwrap())
            .unwrap();
    let recorded_identity = (
        owner.get("ino").and_then(serde_json::Value::as_u64),
        owner.get("dev").and_then(serde_json::Value::as_u64),
    );
    let mut observed = Vec::new();
    let (_process, cleanup) = wait_and_burn_with(
        &plan,
        &user,
        0,
        |_| {
            Ok(OpenProcessResult::Exited {
                code: 0,
                ui_ready: true,
            })
        },
        |root, expected| {
            observed.push((expected.ino, expected.dev));
            burn_session_home(root, expected)
        },
    )
    .unwrap();
    assert!(cleanup.removed());
    assert_eq!(observed.first().copied(), Some(recorded_identity));
}

#[test]
fn native_open_handoff_publishes_the_spawned_process_identity() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let mut plan = prepare_incognito_open(&app, &user, &source, std::process::id() as i32).unwrap();
    plan.debug_port = 0;
    let executable = plan.bin.clone();
    fs::write(
        &executable,
        "#!/bin/sh\n\
for i in $(seq 1 100); do\n\
  if grep -q \"\\\"pid\\\":$$\" \"$INCODEX_SESSION_ROOT/owner.json\"; then\n\
    cat \"$INCODEX_SESSION_ROOT/owner.json\" > \"$INCODEX_SOURCE_HOME/handoff-owner.json\"\n\
    exit 0\n\
  fi\n\
  sleep 0.01\n\
done\n\
cat \"$INCODEX_SESSION_ROOT/owner.json\" > \"$INCODEX_SOURCE_HOME/handoff-owner.json\"\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
    }
    fs::set_permissions(&executable, permissions).unwrap();

    let process = spawn_plan(&plan).unwrap();
    assert!(matches!(process, OpenProcessResult::Exited { code: 0, .. }));
    let owner: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(source.join("handoff-owner.json")).unwrap())
            .unwrap();
    assert_ne!(
        owner.get("pid").and_then(serde_json::Value::as_i64),
        Some(i64::from(std::process::id() as i32)),
        "the session owner must follow the spawned process, not the launcher"
    );
    assert!(owner
        .get("processStartIdentity")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.is_empty()));
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
}

#[test]
fn native_open_keeps_the_handoff_snapshot_when_owner_manifest_changes_after_exit() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let mut plan = prepare_incognito_open(&app, &user, &source, std::process::id() as i32).unwrap();
    plan.debug_port = 0;
    let executable = plan.bin.clone();
    fs::write(
        &executable,
        "#!/bin/sh\n\
for i in $(seq 1 100); do\n\
  if grep -q \"\\\"pid\\\":$$\" \"$INCODEX_SESSION_ROOT/owner.json\"; then\n\
    sed 's/\\\"processStartIdentity\\\":\\\"[^\\\"]*\\\"/\\\"processStartIdentity\\\":\\\"tampered-after-handoff\\\"/' \"$INCODEX_SESSION_ROOT/owner.json\" > \"$INCODEX_SESSION_ROOT/owner.json.tmp\"\n\
    mv \"$INCODEX_SESSION_ROOT/owner.json.tmp\" \"$INCODEX_SESSION_ROOT/owner.json\"\n\
    exit 0\n\
  fi\n\
  sleep 0.01\n\
done\n\
exit 2\n",
    )
    .unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(0o755);
    }
    fs::set_permissions(&executable, permissions).unwrap();

    let (_process, cleanup) = wait_and_burn(&plan, &user, 0).unwrap();
    assert!(
        matches!(cleanup, CleanupResult::Retained { .. }),
        "a post-handoff owner replacement must be rejected by the captured snapshot"
    );
    assert!(plan.session_root.exists());
}

#[test]
fn native_open_marks_pending_until_handoff_and_sweep_retains_it() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let plan = prepare_incognito_open(&app, &user, &source, 999999).unwrap();
    let owner_path = plan.session_root.join("owner.json");
    let owner: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&owner_path).unwrap()).unwrap();
    assert_eq!(
        owner
            .get("handoffPending")
            .and_then(serde_json::Value::as_bool),
        Some(true),
        "only a newly opened session needs conservative pre-handoff retention"
    );
    assert_eq!(sweep_orphan_sessions(&user, None), 0);
    assert!(plan.session_root.exists());
}

#[test]
fn native_open_handoff_clears_pending_atomically() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let plan = prepare_incognito_open(&app, &user, &source, 999999).unwrap();
    handoff_session_owner(&plan.session_root, std::process::id() as i32).unwrap();
    let owner: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(plan.session_root.join("owner.json")).unwrap())
            .unwrap();
    assert_eq!(
        owner
            .get("handoffPending")
            .and_then(serde_json::Value::as_bool),
        Some(false),
        "handoff must publish the owner and clear pending in one atomic record"
    );
}

#[test]
fn failed_handoff_kill_waits_for_a_reaped_child() {
    let mut child = Command::new("/bin/sh")
        .args(["-c", "sleep 30"])
        .spawn()
        .unwrap();
    let pid = child.id();
    let status = kill_and_reap(&mut child).unwrap();
    assert!(!status.success());
    assert!(child.try_wait().unwrap().is_some());
    assert_ne!(
        unsafe { libc::kill(pid as i32, 0) },
        0,
        "a failed handoff must not leave its killed child unreaped"
    );
}

#[test]
fn late_recreation_after_proven_delete_uses_session_path_proof() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
    let mut observed = Vec::new();
    let mut recreated = false;
    let (_process, cleanup) = wait_and_burn_with(
        &plan,
        &user,
        0,
        |_| {
            Ok(OpenProcessResult::Exited {
                code: 0,
                ui_ready: true,
            })
        },
        |session_root, expected| {
            observed.push((expected.ino, expected.dev));
            let result = burn_session_home(session_root, expected);
            if !recreated && result.is_ok() {
                recreated = true;
                fs::create_dir(session_root).unwrap();
                fs::write(session_root.join("late-plugin-cache"), "late\n").unwrap();
            }
            result
        },
    )
    .unwrap();
    assert_eq!(
        observed.get(1).copied(),
        Some((None, None)),
        "late recreation must use path proof only after the original root was deleted"
    );
    assert!(cleanup.removed(), "late recreation must not be retained");
    assert!(!plan.session_root.exists());
}

#[test]
fn async_recreation_after_proven_delete_is_observed_before_removed() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
    let writer_path = plan.session_root.clone();
    let (writer_ready_tx, writer_ready_rx) = mpsc::channel();
    let writer = thread::spawn(move || {
        writer_ready_tx.send(()).unwrap();
        while writer_path.exists() {
            thread::yield_now();
        }
        thread::sleep(Duration::from_millis(2));
        fs::create_dir(&writer_path).unwrap();
        fs::write(writer_path.join("late-plugin-cache"), "late\n").unwrap();
    });
    writer_ready_rx.recv().unwrap();

    let (_process, cleanup) = wait_and_burn_with(
        &plan,
        &user,
        10,
        |_| {
            Ok(OpenProcessResult::Exited {
                code: 0,
                ui_ready: true,
            })
        },
        burn_session_home,
    )
    .unwrap();
    writer.join().unwrap();

    assert!(
        cleanup.removed(),
        "late recreation must be observed and burned"
    );
    assert!(!plan.session_root.exists());
}
