//! [INPUT]: 依赖 open 的退出映射、子进程监督与会话owner契约。
//! [OUTPUT]: 验证退出码、信号、回收和owner handoff。
//! [POS]: open 的进程退出回归；仅启动合成子进程，不触碰官方App。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use super::super::*;
use super::{fake_app, temp_root};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn native_close_burns_only_after_an_observed_window_disappears() {
    use super::{NativeCloseAction as Action, NativeWindowObservation as Window};

    let mut lifecycle = super::NativeCloseLifecycle::new(true).unwrap();
    assert_eq!(lifecycle.observe(Window::Missing), Action::Keep);
    assert_eq!(lifecycle.observe(Window::Present), Action::Keep);
    assert_eq!(lifecycle.observe(Window::Missing), Action::Keep);
    assert_eq!(lifecycle.observe(Window::Missing), Action::Close);
}

#[cfg(unix)]
#[test]
fn native_red_close_reports_a_crashed_official_child_after_burning() {
    use std::os::unix::process::ExitStatusExt;

    let removed = CleanupResult::Removed { attempts: 1 };
    let crash = std::process::ExitStatus::from_raw(libc::SIGTRAP);
    let result =
        super::native_close_process_result(crash, super::NativeCloseExitOrigin::Unrequested);
    assert_eq!(result.exit_code(&removed), OpenExitCode::ProcessFailure);
    assert!(matches!(
        result,
        OpenProcessResult::TerminatedBySignal {
            signal: libc::SIGTRAP
        }
    ));
    assert!(result
        .failure_message(OpenExitCode::ProcessFailure)
        .contains("signal 5"));
    let output = super::format_open_completion(&result, &removed);
    assert!(output.contains("stopped unexpectedly"), "{output}");
    assert!(
        !output.contains("Closed. Isolated session removed."),
        "{output}"
    );

    let clean = std::process::ExitStatus::from_raw(0);
    assert_eq!(
        super::native_close_process_result(clean, super::NativeCloseExitOrigin::Unrequested,)
            .exit_code(&removed),
        OpenExitCode::Success
    );
}

#[cfg(unix)]
#[test]
fn native_red_close_requested_sigterm_is_a_clean_real_child_exit() {
    use std::os::unix::process::ExitStatusExt;

    let removed = CleanupResult::Removed { attempts: 1 };
    let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
    let (status, origin) = super::terminate_and_reap_after_native_close_with_timing(
        &mut child,
        Duration::from_secs(2),
        Duration::from_millis(10),
    )
    .unwrap();
    assert_eq!(status.signal(), Some(libc::SIGTERM));
    assert_eq!(origin, super::NativeCloseExitOrigin::RequestedTerm);

    let result = super::native_close_process_result(status, origin);
    assert_eq!(
        result.exit_code(&removed),
        OpenExitCode::Success,
        "a successfully requested SIGTERM after confirmed red-close is normal shutdown"
    );
}

#[cfg(unix)]
#[test]
fn signals_without_a_native_close_request_remain_process_failures() {
    use std::os::unix::process::ExitStatusExt;

    let removed = CleanupResult::Removed { attempts: 1 };
    for signal in [libc::SIGTERM, libc::SIGTRAP] {
        let mut child = Command::new("/bin/sleep").arg("30").spawn().unwrap();
        assert_eq!(unsafe { libc::kill(child.id() as i32, signal) }, 0);
        let status = child.wait().unwrap();
        assert_eq!(status.signal(), Some(signal));

        assert_eq!(
            super::native_close_process_result(status, super::NativeCloseExitOrigin::Unrequested,)
                .exit_code(&removed),
            OpenExitCode::ProcessFailure,
            "signal {signal} was not requested by the confirmed native close"
        );
    }
}

#[cfg(unix)]
#[test]
fn already_reaped_real_child_does_not_trigger_the_kill_fallback() {
    let mut child = Command::new("/bin/sh")
        .args(["-c", "exit 0"])
        .spawn()
        .unwrap();
    let exited = child.wait().unwrap();
    assert_eq!(exited.code(), Some(0));

    let (status, origin) = super::terminate_and_reap_after_native_close_with_timing(
        &mut child,
        Duration::from_millis(50),
        Duration::from_millis(5),
    )
    .unwrap();
    assert_eq!(
        origin,
        super::NativeCloseExitOrigin::Unrequested,
        "a reaped child must not receive SIGTERM or SIGKILL"
    );
    assert_eq!(status.code(), Some(0));
}

#[cfg(unix)]
#[test]
fn ignored_term_falls_back_to_reaping_a_real_child_after_sigkill() {
    use std::os::unix::process::ExitStatusExt;

    let root = temp_root();
    let ready = root.join("term-ignored-ready");
    let mut child = Command::new("/bin/sh")
        .args([
            "-c",
            "trap '' TERM; : > \"$1\"; exec /bin/sleep 30",
            "incodex-test-child",
        ])
        .arg(&ready)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !ready.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists(), "child must install its ignored-TERM state");
    let (status, origin) = super::terminate_and_reap_after_native_close_with_timing(
        &mut child,
        Duration::from_millis(100),
        Duration::from_millis(10),
    )
    .unwrap();
    assert_eq!(status.signal(), Some(libc::SIGKILL));
    assert_eq!(origin, super::NativeCloseExitOrigin::RequestedKill);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejected_ui_keeps_the_existing_clean_session_removal_message() {
    let removed = CleanupResult::Removed { attempts: 1 };
    let rejected = OpenProcessResult::Exited {
        code: 0,
        ui_ready: false,
    };
    assert_eq!(
        rejected.exit_code(&removed),
        OpenExitCode::UiInjectionFailure
    );
    assert!(
        super::format_open_completion(&rejected, &removed)
            .contains("Closed. Isolated session removed."),
        "UI rejection must not be described as a process crash"
    );
}

#[cfg(unix)]
#[test]
fn child_exit_before_window_observation_retains_the_fatal_signal() {
    use std::os::unix::process::ExitStatusExt;

    let (_sender, statuses) = mpsc::channel();
    let readiness = AtomicBool::new(true);
    let crash = std::process::ExitStatus::from_raw(libc::SIGTRAP);
    assert!(matches!(
        super::completed_process_result(crash, &readiness, &statuses),
        OpenProcessResult::TerminatedBySignal {
            signal: libc::SIGTRAP
        }
    ));
}

#[cfg(unix)]
#[test]
fn a_kill_sent_by_the_owned_red_close_fallback_is_not_reported_as_a_crash() {
    use std::os::unix::process::ExitStatusExt;

    let removed = CleanupResult::Removed { attempts: 1 };
    let forced = std::process::ExitStatus::from_raw(libc::SIGKILL);
    let result =
        super::native_close_process_result(forced, super::NativeCloseExitOrigin::RequestedKill);
    assert_eq!(result.exit_code(&removed), OpenExitCode::Success);

    let spontaneous = std::process::ExitStatus::from_raw(libc::SIGTRAP);
    let result =
        super::native_close_process_result(spontaneous, super::NativeCloseExitOrigin::Unrequested);
    assert_eq!(result.exit_code(&removed), OpenExitCode::ProcessFailure);
    let result = super::native_close_process_result(
        spontaneous,
        super::NativeCloseExitOrigin::RequestedKill,
    );
    assert_eq!(result.exit_code(&removed), OpenExitCode::ProcessFailure);
}

#[test]
fn minimize_and_uncertain_window_state_never_burn_a_live_session() {
    use super::{NativeCloseAction as Action, NativeWindowObservation as Window};

    let mut lifecycle = super::NativeCloseLifecycle::new(true).unwrap();
    assert_eq!(lifecycle.observe(Window::Present), Action::Keep);
    assert_eq!(lifecycle.observe(Window::Minimized), Action::Keep);
    assert_eq!(lifecycle.observe(Window::Missing), Action::Keep);
    assert_eq!(lifecycle.observe(Window::Unknown), Action::Keep);
    assert_eq!(lifecycle.observe(Window::Missing), Action::Keep);
    assert_eq!(lifecycle.observe(Window::Present), Action::Keep);
}

#[test]
fn native_open_plan_claims_native_cleanup_ownership() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();

    let plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
    assert!(plan
        .env
        .iter()
        .any(|(key, value)| key == "INCODEX_CLEANUP_OWNER" && value == "native"));
    assert!(
        plan.env
            .iter()
            .any(|(key, value)| key == "INCODEX_NATIVE_OPEN" && value == "1"),
        "macOS native open must exclusively own startup as well as cleanup"
    );
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
    fs::remove_dir_all(root).unwrap();
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
fn owned_child_reap_reports_when_it_sent_the_fallback_kill() {
    let mut child = Command::new("/bin/sh")
        .args(["-c", "sleep 30"])
        .spawn()
        .unwrap();
    let (status, forced) = super::kill_and_reap_with_origin(&mut child).unwrap();
    assert!(forced);
    assert!(!status.success());
    assert!(child.try_wait().unwrap().is_some());
}
