// Cleanup-focused `open` tests. Split from open_tests.rs to keep each source
// file below the repository's size budget.
use super::open_tests::{fake_app, temp_root};
use super::*;
use crate::profile_mask::{ProfileAvatar, ProfileMask};
use std::fs;
use std::time::{Duration, Instant};

#[test]
fn wait_and_burn_quiesces_session_helpers_before_first_burn() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
    let events = std::cell::RefCell::new(Vec::new());

    let (_process, cleanup) = wait_and_burn_with_owner(
        &plan,
        &user,
        0,
        |_| {
            Ok(SpawnOutcome {
                process: OpenProcessResult::Exited {
                    code: 0,
                    ui_ready: true,
                },
                owner: None,
                cleanup: CleanupDisposition::Burn,
            })
        },
        |session_root| {
            assert_eq!(session_root, plan.session_root);
            events.borrow_mut().push("quiesce");
            Ok(())
        },
        |session_root, expected, _owner| {
            events.borrow_mut().push("burn");
            burn_session_home(session_root, expected)
        },
    )
    .unwrap();

    assert!(cleanup.removed());
    let events = events.into_inner();
    assert_eq!(events.first(), Some(&"quiesce"));
    assert!(events[1..].iter().all(|event| *event == "burn"));
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
    let (_process, cleanup) =
        wait_and_burn_with(&plan, &user, 0, |_| Err("ENOENT".into()), burn_session_home).unwrap();
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
    fs::write(&plan.bin, "#!/bin/sh\nsleep 3\n").unwrap();
    plan.debug_port = 0;
    plan.profile_mask = Some(ProfileMask {
        name: "Temporary".into(),
        avatar: ProfileAvatar::Generated,
    });
    let started = Instant::now();
    let process = spawn_plan(&plan).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "a rejected UI must close the child instead of leaving the window visible"
    );
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
fn unmasked_cdp_failure_keeps_the_existing_window_lifecycle() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let mut plan = prepare_incognito_open(&app, &user, &source, 1).unwrap();
    fs::write(&plan.bin, "#!/bin/sh\nsleep 0.2\n").unwrap();
    plan.debug_port = 0;

    let started = Instant::now();
    let process = spawn_plan(&plan).unwrap();

    assert!(
        started.elapsed() >= Duration::from_millis(100),
        "ordinary open keeps its established best-effort injection lifecycle"
    );
    assert_eq!(
        process.exit_code(&CleanupResult::Removed { attempts: 1 }),
        OpenExitCode::UiInjectionFailure
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

#[test]
fn profile_mask_is_carried_from_open_preparation_to_the_plan() {
    let root = temp_root();
    let app = fake_app(&root);
    let user = root.join("home");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{}\n").unwrap();
    let profile_mask = ProfileMask {
        name: "Temporary".into(),
        avatar: ProfileAvatar::Generated,
    };

    let plan = prepare_incognito_open_with_profile_mask(
        &app,
        &user,
        &source,
        1,
        Some(profile_mask.clone()),
    )
    .unwrap();

    assert_eq!(plan.profile_mask, Some(profile_mask));
}
