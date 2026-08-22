// Tests for the native `open` lifecycle. Kept outside open.rs so the product
// implementation remains below the repository's per-file size budget.
use super::*;
use std::fs;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn temp_root() -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("incodex-open-unit-{n}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn fake_app(root: &Path) -> PathBuf {
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
fn open_progress_distinguishes_launch_ready_and_waiting() {
    let (opening, opened, waiting) = open_progress_copy();
    assert_eq!(opening, "Opening incognito Codex window");
    assert_eq!(opened, "Opened. Incognito Codex window is ready.");
    assert_eq!(waiting, "Waiting for the window to close");
}

#[test]
fn ready_published_between_status_poll_and_child_exit_is_not_lost() {
    let (status_tx, status_rx) = mpsc::channel();
    let readiness = InjectionReadiness::default();

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
        readiness.is_ready(),
        "Ready published between lifecycle polls must survive child exit"
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
    let mut plan =
        prepare_incognito_open(&app, &user, &source, std::process::id() as i32).unwrap();
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
        |session_root, expected| burn_session_home(session_root, expected),
    )
    .unwrap();
    writer.join().unwrap();

    assert!(cleanup.removed(), "late recreation must be observed and burned");
    assert!(!plan.session_root.exists());
}

#[test]
fn spawn_error_still_burns() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
    let (_process, cleanup) = wait_and_burn_with(
        &plan,
        &user,
        0,
        |_| Err("ENOENT".into()),
        |root, expected| burn_session_home(root, expected),
    )
    .unwrap();
    assert!(!plan.session_root.exists());
    assert!(cleanup.removed());
}

#[test]
fn cdp_port_failure_is_not_success() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let mut plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
    plan.debug_port = 0;
    let process = spawn_plan(&plan).unwrap();
    assert_eq!(
        process.exit_code(&CleanupResult::Removed { attempts: 1 }),
        OpenExitCode::UiInjectionFailure,
        "missing CDP port must be a UI acceptance failure"
    );
    burn_session_home(
        &plan.session_root,
        &BurnExpected {
            user_root: &user,
            session_id: Some(&plan.session_id),
            ino: None,
            dev: None,
        },
    )
    .unwrap();
}

#[test]
fn locale_override_is_carried_into_the_cdp_injection_plan() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("config.toml"),
        "model = \"test\"\nlocaleOverride = \"zh-CN\"\n",
    )
    .unwrap();
    let plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
    assert_eq!(plan.locale.as_deref(), Some("zh-CN"));
}
