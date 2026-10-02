// 登录观察者的订阅、取消、授权与提前挂钩回归；测试不修改官方包。
use super::*;
use std::cell::RefCell;

#[test]
fn update_prearms_the_exact_target_before_registration_completes() {
    let observation = crate::windows_update_repair::PackageUpdateObservation {
        source_package_full_name: "OpenAI.Codex_1.2.3.4_x64__2p2nqsd0c76g0".into(),
        target_package_full_name: "OpenAI.Codex_1.2.3.5_x64__2p2nqsd0c76g0".into(),
        target_package_family_name: CODEX_PACKAGE_FAMILY_NAME.into(),
        complete: false,
        error_code: 0,
    };
    let calls = RefCell::new(Vec::new());
    handle_package_update_with(
        &observation,
        |target| {
            assert_eq!(
                target.target_package_full_name,
                observation.target_package_full_name
            );
            calls.borrow_mut().push("prearm");
            Ok(())
        },
        || calls.borrow_mut().push("reconcile"),
    )
    .unwrap();
    assert_eq!(
        *calls.borrow(),
        ["prearm"],
        "waiting for completion misses the first automatic launch"
    );
}

#[test]
fn update_completion_attempts_prearm_before_waking_the_fallback() {
    let observation = crate::windows_update_repair::PackageUpdateObservation {
        source_package_full_name: "OpenAI.Codex_1.2.3.4_x64__2p2nqsd0c76g0".into(),
        target_package_full_name: "OpenAI.Codex_1.2.3.5_x64__2p2nqsd0c76g0".into(),
        target_package_family_name: CODEX_PACKAGE_FAMILY_NAME.into(),
        complete: true,
        error_code: 0,
    };
    let calls = RefCell::new(Vec::new());
    let result = handle_package_update_with(
        &observation,
        |_| {
            calls.borrow_mut().push("prearm");
            Err("early preparation unavailable".into())
        },
        || calls.borrow_mut().push("reconcile"),
    );
    assert!(result.is_err());
    assert_eq!(
        *calls.borrow(),
        ["prearm", "reconcile"],
        "failed prearm must retain the completed-event fallback"
    );
}

#[test]
fn observer_launch_escapes_an_inherited_installer_job() {
    use windows_sys::Win32::System::Threading::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW};

    let flags = observer_creation_flags();
    assert_ne!(flags & CREATE_NO_WINDOW, 0);
    assert_ne!(
        flags & CREATE_BREAKAWAY_FROM_JOB,
        0,
        "the login observer must outlive an installer launched inside a permitted Job"
    );
}

#[test]
fn observer_read_does_not_create_a_missing_root() {
    let root = fixture_root("missing-root");
    assert!(!root.exists());
    let (state, intent) = read_observer_state(&root).unwrap();
    assert!(state.is_none() && intent.is_none());
    assert!(!root.exists());
}

#[test]
fn observer_reseals_root_after_sandbox_adds_a_read_ace() {
    let root = fixture_root("sandbox-read");
    incodex_core::windows_session::ensure_private_windows_dir(&root).unwrap();
    let grant = std::process::Command::new("icacls")
        .arg(&root)
        .args(["/grant", "*S-1-5-32-545:(RX)"])
        .output()
        .unwrap();
    assert!(
        grant.status.success(),
        "{}",
        String::from_utf8_lossy(&grant.stderr)
    );
    assert!(incodex_core::windows_session::verify_private_acl(&root).is_err());

    let (state, intent) = read_observer_state(&root).unwrap();
    assert!(state.is_none() && intent.is_none());
    incodex_core::windows_session::verify_private_acl(&root).unwrap();
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn same_generation_startup_reapplies_registration_without_republishing_runtime() {
    let root = fixture_root("rearm");
    let state = crate::windows_install::install_windows_runtime_with(
        &root,
        "OpenAI.Codex_1.2.3.4_x64__2p2nqsd0c76g0",
        &std::env::current_exe().unwrap(),
        |_| Ok(vec![]),
        |_| Ok(false),
        |_| Ok(()),
        |_| Ok(()),
    )
    .unwrap();
    let before = std::fs::read(root.join("runtime/current.json")).unwrap();
    let mut calls = 0;
    rearm_current_registration_with(
        &root,
        &state,
        |_| Ok(vec![]),
        |_| {
            calls += 1;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        calls, 1,
        "same package on disk does not prove the OS activation hook survived login"
    );
    assert_eq!(
        std::fs::read(root.join("runtime/current.json")).unwrap(),
        before
    );
    assert_eq!(
        read_windows_install_state(&root).unwrap(),
        Some(state.clone())
    );
    assert!(rearm_current_registration_with(
        &root,
        &state,
        |_| Ok(vec![42]),
        |_| panic!("must wait for normal exit")
    )
    .is_err());
    assert!(rearm_current_registration_with(
        &root,
        &state,
        |_| Ok(vec![]),
        |_| Err("registration failed".into())
    )
    .is_err());
    let mut probes = 0;
    let raced = rearm_current_registration_with(
        &root,
        &state,
        |_| {
            probes += 1;
            Ok(if probes == 1 { vec![] } else { vec![42] })
        },
        |_| Ok(()),
    );
    assert!(
        raced.is_err(),
        "launch during rearm must return to process waiting"
    );
}

#[test]
fn observer_status_retains_bounded_history_without_duplicate_idle_events() {
    let root = fixture_root("history");
    incodex_core::windows_session::ensure_private_windows_dir(&root).unwrap();
    status(&root, "subscribed", "startup").unwrap();
    status(&root, "watching", "package-A").unwrap();
    status(&root, "watching", "package-A").unwrap();
    let path = root.join("windows/update-observer.json");
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let events = value["events"]
        .as_array()
        .expect("history must survive status replacement");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["phase"], "subscribed");
    for index in 0..160 {
        status(
            &root,
            "repairing",
            &format!("{index}:{}", "测试\n".repeat(2000)),
        )
        .unwrap();
    }
    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.len() <= 65536);
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let events = value["events"].as_array().unwrap();
    assert!(events.len() <= 128);
    assert!(events.last().unwrap()["detail"]
        .as_str()
        .unwrap()
        .starts_with("159:"));
}

fn fixture_root(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "incodex-observer-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn observer_has_only_one_owner_and_releases_it_on_exit() {
    let root = fixture_root("owner");
    let first = acquire_owner(&root).unwrap().unwrap();
    let second_root = root.clone();
    assert!(
        std::thread::spawn(move || acquire_owner(&second_root).unwrap().is_none())
            .join()
            .unwrap()
    );
    drop(first);
    assert!(acquire_owner(&root).unwrap().is_some());
}

#[test]
fn cancellation_takes_priority_over_an_update_or_process_exit() {
    let root = fixture_root("cancel");
    let stop = event(&root, "stop", true).unwrap();
    let wake = event(&root, "wake", false).unwrap();
    unsafe {
        SetEvent(stop.0);
        SetEvent(wake.0);
    }
    assert!(!wait(&stop, &wake).unwrap());
}

#[test]
fn absent_authorization_stops_before_waiting_for_another_event() {
    run_observer_with(
        || Ok(()),
        || Ok(false),
        || panic!("cancelled observer waited again"),
    )
    .unwrap();
}

#[test]
fn package_unavailable_at_login_keeps_subscription_until_recovery_event() {
    let mut reconciliations = 0;
    let mut wakeups = 0;
    let result = run_observer_with(
        || Ok(()),
        || {
            reconciliations += 1;
            match reconciliations {
                1 => Err("official Codex Microsoft Store package is not healthy".into()),
                2 => Ok(true),
                _ => Ok(false),
            }
        },
        || {
            wakeups += 1;
            Ok(true)
        },
    );
    assert!(
        result.is_ok(),
        "temporary package failure ended observer: {result:?}"
    );
    assert_eq!(reconciliations, 3);
    assert_eq!(wakeups, 2);
}

#[test]
fn unavailable_package_can_be_cancelled_without_retry_or_mutation() {
    let mut attempts = 0;
    let result = run_observer_with(
        || Ok(()),
        || {
            attempts += 1;
            Err("package unavailable".into())
        },
        || Ok(false),
    );
    assert!(result.is_ok());
    assert_eq!(attempts, 1);
}

#[test]
fn observer_subscribes_before_startup_reconciliation_without_a_new_event() {
    let calls = RefCell::new(Vec::new());
    run_observer_with(
        || {
            calls.borrow_mut().push("subscribe");
            Ok(())
        },
        || {
            calls.borrow_mut().push("reconcile");
            Ok(true)
        },
        || {
            calls.borrow_mut().push("stop");
            Ok(false)
        },
    )
    .unwrap();
    assert_eq!(*calls.borrow(), ["subscribe", "reconcile", "stop"]);
}

#[test]
fn observer_reconciles_every_generation_and_stops_without_authorization() {
    let mut reconciliations = 0;
    let mut wakeups = 0;
    run_observer_with(
        || Ok(()),
        || {
            reconciliations += 1;
            Ok(reconciliations < 3)
        },
        || {
            wakeups += 1;
            Ok(true)
        },
    )
    .unwrap();
    assert_eq!(reconciliations, 3);
    assert_eq!(
        wakeups, 2,
        "startup reconciliation must not require an event"
    );
}

#[test]
fn explicit_stop_wakes_an_idle_observer_and_waits_for_its_owner() {
    let root = fixture_root("stop");
    let worker_root = root.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _owner = acquire_owner(&worker_root).unwrap().unwrap();
        let cancelled = event(&worker_root, "stop", true).unwrap();
        let wake = event(&worker_root, "wake", false).unwrap();
        sender.send(()).unwrap();
        assert!(!wait(&cancelled, &wake).unwrap());
    });
    receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    stop(&root).unwrap();
    worker.join().unwrap();
    assert!(acquire_owner(&root).unwrap().is_some());
}

#[test]
fn family_subscription_can_be_established_without_an_app_owner() {
    let _apartment = WindowsRuntimeApartment::initialize().unwrap();
    let wake = event(&fixture_root("subscribe"), "wake", false).unwrap();
    let subscription = subscribe(wake, None).unwrap();
    drop(subscription);
}
