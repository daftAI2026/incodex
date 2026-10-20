// ============================================================================
// session_tests: 用真实文件系统验证 session 身份边界与销毁行为
// ============================================================================
// [INPUT]: 依赖 session 模块的创建、owner 校验和销毁 API
// [OUTPUT]: 验证只读后代可销毁，symlink 目标与错误身份保持不变
// [POS]: incodex-core/session 的 Unix 回归测试，只写自有临时目录
// [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = TEMP_ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("incodex-session-{pid}-{n}-{counter}"));
    fs::create_dir(&dir).unwrap();
    dir
}

fn expected_burn<'a>(session: &'a SessionHome, user_root: &'a Path) -> BurnExpected<'a> {
    BurnExpected {
        user_root,
        session_id: Some(&session.session_id),
        ino: Some(session.ino),
        dev: Some(session.dev),
    }
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
}

fn skip_if_root(test_name: &str) -> bool {
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("SKIP {test_name}: root bypasses Unix directory write permissions");
        true
    } else {
        false
    }
}

#[test]
fn create_session_uses_random_directory_under_sessions() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let first = create_session_home(&user_root, Some("t1"), 1, "").unwrap();
    let second = create_session_home(&user_root, Some("t1"), 1, "").unwrap();
    assert_ne!(first.home, second.home);
    assert_ne!(first.session_id, second.session_id);
    let sessions = fs::canonicalize(user_root.join("sessions")).unwrap();
    assert!(first.root.starts_with(sessions));
    assert!(first.home.ends_with("codex-home"));
    assert!(first.chromium.ends_with("chromium"));
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&user_root), 0o700);
    assert_eq!(mode(&first.root), 0o700);
    assert_eq!(mode(&first.root.join("owner.json")), 0o600);
}

#[test]
fn copy_settings_then_burn_removes_the_session() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{\"token\":\"x\"}").unwrap();
    let session = create_session_home(&user_root, None, 0, "").unwrap();
    assert_eq!(copy_settings(&session.home, &source).unwrap(), 1);
    assert_eq!(
        fs::read_to_string(session.home.join("auth.json")).unwrap(),
        "{\"token\":\"x\"}"
    );
    burn_session_home(
        &session.root,
        &BurnExpected {
            user_root: &user_root,
            session_id: Some(&session.session_id),
            ino: Some(session.ino),
            dev: Some(session.dev),
        },
    )
    .unwrap();
    assert!(!session.root.exists());
    assert_eq!(
        fs::read_to_string(source.join("auth.json")).unwrap(),
        "{\"token\":\"x\"}"
    );
}

#[test]
fn window_state_seed_projects_stable_geometry_and_fresh_home_markers() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join(".codex-global-state.json"),
        r#"{
          "electron-main-window-bounds": {
            "x": -40,
            "y": 38,
            "width": 1710,
            "height": 1073,
            "isMaximized": true
          },
          "thread-titles": {"secret-thread": "must-not-cross"},
          "selected-project": "/private/project",
          "electron-persisted-atom-state": {"secret": true}
        }"#,
    )
    .unwrap();
    let session = create_session_home(&user_root, None, 0, "").unwrap();

    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert!(seed_window_state(&session.home, &source).unwrap());
    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    let destination = session.home.join(".codex-global-state.json");
    let mut state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&destination).unwrap()).unwrap();
    let first_seen = state
        .as_object_mut()
        .unwrap()
        .remove("desktop-first-seen-at-ms")
        .and_then(|value| value.as_u64())
        .expect("seeded state must preserve official fresh-home initialization");
    assert!((before..=after).contains(&first_seen));
    assert_eq!(
        state,
        serde_json::json!({
            "electron-main-window-bounds": {
                "x": -40,
                "y": 38,
                "width": 1710,
                "height": 1073
            },
            "electron-persisted-atom-state": {
                "chatgpt-migration-announcement-completed-v1": true,
                "chatgpt-update-downloaded-announcement-seen-v1": true
            }
        })
    );
    assert_eq!(
        fs::metadata(destination).unwrap().permissions().mode() & 0o777,
        FILE_MODE
    );
}

#[test]
fn window_state_seed_prefers_live_geometry_over_stale_persisted_bounds() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join(".codex-global-state.json"),
        r#"{
          "electron-main-window-bounds": {
            "x": 0,
            "y": 38,
            "width": 1710,
            "height": 1073,
            "isMaximized": true
          }
        }"#,
    )
    .unwrap();
    let session = create_session_home(&user_root, None, 0, "").unwrap();

    let live = WindowGeometry {
        x: 597,
        y: 34,
        width: 869,
        height: 1073,
    };
    assert!(seed_window_state_with_geometry(&session.home, &source, Some(live)).unwrap());

    let state: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(session.home.join(".codex-global-state.json")).unwrap(),
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
fn window_state_seed_ignores_missing_or_malformed_bounds_without_touching_settings() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{\"token\":\"source\"}\n").unwrap();
    fs::write(source.join("config.toml"), "localeOverride = \"zh-CN\"\n").unwrap();
    fs::write(
        source.join(".codex-global-state.json"),
        r#"{"electron-main-window-bounds":{"x":0,"y":0,"width":"wide","height":820,"isMaximized":false}}"#,
    )
    .unwrap();
    let session = create_session_home(&user_root, None, 0, "").unwrap();

    assert_eq!(copy_settings(&session.home, &source).unwrap(), 2);
    assert!(!seed_window_state(&session.home, &source).unwrap());

    assert!(!session.home.join(".codex-global-state.json").exists());
    assert_eq!(
        fs::read_to_string(session.home.join("config.toml")).unwrap(),
        "localeOverride = \"zh-CN\"\n"
    );
    assert_eq!(
        fs::read_to_string(session.home.join("auth.json")).unwrap(),
        "{\"token\":\"source\"}\n"
    );
}

#[test]
fn window_state_seed_refuses_a_symlink_source() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    let outside = root.join("outside-global-state.json");
    fs::write(
        &outside,
        r#"{"electron-main-window-bounds":{"x":0,"y":0,"width":900,"height":700,"isMaximized":false}}"#,
    )
    .unwrap();
    std::os::unix::fs::symlink(&outside, source.join(".codex-global-state.json")).unwrap();
    let session = create_session_home(&user_root, None, 0, "").unwrap();

    assert!(seed_window_state(&session.home, &source).is_err());
    assert!(!session.home.join(".codex-global-state.json").exists());
}

#[test]
fn session_lifecycle_does_not_create_or_mutate_identity_cache() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let source = root.join("codex");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), "{\"token\":\"source\"}\n").unwrap();
    fs::write(source.join("config.toml"), "localeOverride = \"zh-CN\"\n").unwrap();
    let identity = user_root.join("identity");
    fs::create_dir_all(&identity).unwrap();
    fs::write(identity.join("auth.json"), "legacy-cache\n").unwrap();

    let source_before = (
        fs::read(source.join("auth.json")).unwrap(),
        fs::read(source.join("config.toml")).unwrap(),
    );
    let session = create_session_home(&user_root, None, 0, "").unwrap();
    assert_eq!(copy_settings(&session.home, &source).unwrap(), 2);
    assert_eq!(
        fs::read(identity.join("auth.json")).unwrap(),
        b"legacy-cache\n"
    );
    assert_eq!(
        fs::read(session.home.join("auth.json")).unwrap(),
        source_before.0
    );
    assert_eq!(
        fs::read(session.home.join("config.toml")).unwrap(),
        source_before.1
    );

    burn_session_home(
        &session.root,
        &BurnExpected {
            user_root: &user_root,
            session_id: Some(&session.session_id),
            ino: Some(session.ino),
            dev: Some(session.dev),
        },
    )
    .unwrap();
    assert_eq!(fs::read(source.join("auth.json")).unwrap(), source_before.0);
    assert_eq!(
        fs::read(source.join("config.toml")).unwrap(),
        source_before.1
    );
    assert_eq!(
        fs::read(identity.join("auth.json")).unwrap(),
        b"legacy-cache\n"
    );
}

#[test]
fn orphan_sweep_refuses_a_replaced_session_root_without_recorded_identity() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let session = create_session_home(&user_root, None, 999999, "").unwrap();
    fs::remove_dir_all(&session.root).unwrap();
    fs::create_dir(&session.root).unwrap();
    fs::write(session.root.join("replacement.txt"), "keep-me").unwrap();

    assert_eq!(sweep_orphan_sessions(&user_root, None), 0);
    assert!(session.root.exists());
    assert_eq!(
        fs::read_to_string(session.root.join("replacement.txt")).unwrap(),
        "keep-me"
    );
}

#[test]
fn session_owner_records_process_start_identity() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let pid = std::process::id() as i32;
    let session = create_session_home(&user_root, None, pid, "").unwrap();
    let owner: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(session.root.join(OWNER_NAME)).unwrap()).unwrap();
    let expected = process_start_identity(pid).expect("current process identity");
    assert_eq!(
        owner.get("pid").and_then(serde_json::Value::as_i64),
        Some(i64::from(pid))
    );
    assert_eq!(
        owner
            .get("processStartIdentity")
            .and_then(serde_json::Value::as_str),
        Some(expected.as_str()),
        "session owner must use the same start identity source as Runtime owner records"
    );
}

#[test]
fn orphan_sweep_treats_a_reused_pid_as_orphan() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let pid = std::process::id() as i32;
    let session = create_session_home(&user_root, None, pid, "").unwrap();
    let owner_path = session.root.join(OWNER_NAME);
    let mut owner: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&owner_path).unwrap()).unwrap();
    owner["processStartIdentity"] = serde_json::json!("Fri Aug 22 10:37:03 2025");
    fs::write(&owner_path, format!("{owner}\n")).unwrap();

    assert_eq!(sweep_orphan_sessions(&user_root, None), 1);
    assert!(!session.root.exists());
}

#[test]
fn live_session_without_process_start_identity_is_not_swept() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let pid = std::process::id() as i32;
    let session = create_session_home(&user_root, None, pid, "").unwrap();
    let owner_path = session.root.join(OWNER_NAME);
    let mut owner: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&owner_path).unwrap()).unwrap();
    owner
        .as_object_mut()
        .unwrap()
        .remove("processStartIdentity");
    fs::write(&owner_path, format!("{owner}\n")).unwrap();

    assert_eq!(sweep_orphan_sessions(&user_root, None), 0);
    assert!(session.root.exists());
}

#[test]
fn orphan_sweep_retains_a_live_session_when_identity_probe_is_unknown() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let pid = std::process::id() as i32;
    let session = create_session_home(&user_root, None, pid, "").unwrap();

    assert_eq!(
        sweep_orphan_sessions_with_probe(&user_root, None, |_| ProcessProbe::Unknown),
        0
    );
    assert!(session.root.exists());
}

#[test]
fn orphan_sweep_retains_an_unparseable_legacy_process_identity() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let pid = std::process::id() as i32;
    let session = create_session_home(&user_root, None, pid, "").unwrap();
    let owner_path = session.root.join(OWNER_NAME);
    let mut owner: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&owner_path).unwrap()).unwrap();
    owner["processStartIdentity"] = serde_json::json!("六  8月/22 10:37:03 2026");
    fs::write(&owner_path, format!("{owner}\n")).unwrap();

    assert_eq!(
        sweep_orphan_sessions_with_probe(&user_root, None, |_| {
            ProcessProbe::Live("Sat Aug 22 10:37:03 2026".into())
        }),
        0,
        "a locale-dependent legacy identity is unverifiable, not a PID-reuse mismatch"
    );
    assert!(session.root.exists());
}

#[test]
fn orphan_sweep_retains_a_live_session_with_non_c_locale_identity_words() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let pid = std::process::id() as i32;
    let session = create_session_home(&user_root, None, pid, "").unwrap();
    let owner_path = session.root.join(OWNER_NAME);
    let mut owner: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&owner_path).unwrap()).unwrap();
    owner["processStartIdentity"] = serde_json::json!("Sab Ago 22 10:37:03 2025");
    fs::write(&owner_path, format!("{owner}\n")).unwrap();

    assert_eq!(
        sweep_orphan_sessions_with_probe(&user_root, None, |_| {
            ProcessProbe::Live("Sat Aug 22 10:37:03 2025".into())
        }),
        0,
        "a non-C locale identity is unverifiable, not a PID-reuse mismatch"
    );
    assert!(session.root.exists());
}

#[test]
fn process_identity_probe_pins_the_c_locale() {
    assert!(
        include_str!("session.rs").contains(".env(\"LC_ALL\", \"C\")"),
        "owner identity must have one locale-independent ps representation"
    );
}

#[test]
fn burn_revalidates_the_owner_snapshot_before_delete() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let pid = std::process::id() as i32;
    let session = create_session_home(&user_root, None, pid, "").unwrap();
    let start = process_start_identity(pid).unwrap();
    let snapshot = SessionOwnerSnapshot {
        pid,
        process_start_identity: start,
    };
    let owner_path = session.root.join(OWNER_NAME);
    let mut owner: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&owner_path).unwrap()).unwrap();
    owner["pid"] = serde_json::json!(999999);
    fs::write(&owner_path, format!("{owner}\n")).unwrap();

    let error = burn_session_home_with_owner(
        &session.root,
        &BurnExpected {
            user_root: &user_root,
            session_id: Some(&session.session_id),
            ino: Some(session.ino),
            dev: Some(session.dev),
        },
        &snapshot,
    )
    .unwrap_err();
    assert!(error.contains("owner"));
    assert!(session.root.exists());
}

#[test]
fn session_owner_handoff_records_the_child_process_identity() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let session = create_session_home(&user_root, None, 999999, "").unwrap();
    let pid = std::process::id() as i32;
    handoff_session_owner(&session.root, pid).unwrap();
    let owner: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(session.root.join(OWNER_NAME)).unwrap()).unwrap();
    assert_eq!(
        owner.get("pid").and_then(serde_json::Value::as_i64),
        Some(i64::from(pid))
    );
    assert_eq!(
        owner
            .get("processStartIdentity")
            .and_then(serde_json::Value::as_str),
        process_start_identity(pid).as_deref()
    );
}

#[test]
fn burn_refuses_a_session_id_mismatch() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let session = create_session_home(&user_root, None, 0, "").unwrap();
    let err = burn_session_home(
        &session.root,
        &BurnExpected {
            user_root: &user_root,
            session_id: Some("other"),
            ino: None,
            dev: None,
        },
    )
    .unwrap_err();
    assert!(err.contains("mismatch"));
    assert!(session.root.exists());
}

#[test]
fn burn_removes_real_nested_read_only_runtime_generation_tree() {
    if skip_if_root("burn_removes_real_nested_read_only_runtime_generation_tree") {
        return;
    }

    let root = temp_root();
    let user_root = root.join(".incodex");
    let session = create_session_home(&user_root, None, 0, "").unwrap();
    let runtime = session
        .chromium
        .join("artifact-sessions/runtime-generations/generation-X/runtime");
    fs::create_dir_all(&runtime).unwrap();

    let mut read_only_dirs = Vec::new();
    for index in 0..43 {
        let dir = runtime.join(format!("artifact-{index:02}"));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("runtime.bin"), b"private runtime fixture").unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
        read_only_dirs.push(dir);
    }

    let result = burn_session_home(&session.root, &expected_burn(&session, &user_root));
    let removed = matches!(result, Ok(true));
    if !removed {
        for dir in &read_only_dirs {
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }
        let _ = fs::remove_dir_all(&root);
    }
    assert!(
        removed,
        "burn must remove 43 real 0500 descendants: {result:?}"
    );
    assert!(!session.root.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn burn_unlinks_external_symlink_without_changing_target_permissions_or_content() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let session = create_session_home(&user_root, None, 0, "").unwrap();
    let external = root.join("outside-runtime");
    fs::create_dir(&external).unwrap();
    let payload = external.join("runtime.bin");
    fs::write(&payload, b"outside target must survive").unwrap();
    fs::set_permissions(&payload, fs::Permissions::from_mode(0o400)).unwrap();
    fs::set_permissions(&external, fs::Permissions::from_mode(0o500)).unwrap();
    let link = session.chromium.join("external-runtime");
    std::os::unix::fs::symlink(&external, &link).unwrap();

    assert!(burn_session_home(&session.root, &expected_burn(&session, &user_root)).unwrap());

    assert!(!link.exists());
    assert_eq!(fs::read(&payload).unwrap(), b"outside target must survive");
    assert_eq!(mode(&external), 0o500);
    assert_eq!(mode(&payload), 0o400);
    fs::set_permissions(&payload, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&external, fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn burn_rejects_root_inode_mismatch_without_changing_descendant_mode() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let session = create_session_home(&user_root, None, 0, "").unwrap();
    let protected = session.home.join("readonly-child");
    fs::create_dir(&protected).unwrap();
    fs::set_permissions(&protected, fs::Permissions::from_mode(0o500)).unwrap();
    let mut expected = expected_burn(&session, &user_root);
    expected.ino = Some(session.ino.wrapping_add(1));

    let error = burn_session_home(&session.root, &expected).unwrap_err();

    assert!(error.contains("inode"));
    assert!(session.root.exists());
    assert_eq!(mode(&protected), 0o500);
    fs::set_permissions(&protected, fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn burn_rejects_owner_mismatch_without_changing_descendant_mode() {
    let root = temp_root();
    let user_root = root.join(".incodex");
    let pid = std::process::id() as i32;
    let session = create_session_home(&user_root, None, pid, "").unwrap();
    let protected = session.chromium.join("readonly-child");
    fs::create_dir(&protected).unwrap();
    fs::set_permissions(&protected, fs::Permissions::from_mode(0o500)).unwrap();
    let owner = SessionOwnerSnapshot {
        pid: pid + 1,
        process_start_identity: process_start_identity(pid).unwrap(),
    };

    let error =
        burn_session_home_with_owner(&session.root, &expected_burn(&session, &user_root), &owner)
            .unwrap_err();

    assert!(error.contains("owner"));
    assert!(session.root.exists());
    assert_eq!(mode(&protected), 0o500);
    fs::set_permissions(&protected, fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn burn_failure_after_partial_removal_keeps_identity_for_retry() {
    if skip_if_root("burn_failure_after_partial_removal_keeps_identity_for_retry") {
        return;
    }

    let root = temp_root();
    let user_root = root.join(".incodex");
    let session = create_session_home(&user_root, None, 0, "").unwrap();
    let entries = fs::read_dir(&session.root)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| {
            matches!(
                entry.file_name().to_str(),
                Some("codex-home") | Some("chromium")
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), 2);

    // 先选目录项，再把只读障碍放入后一项，确保真实递归删除先删掉标记。
    let removed_first = entries[0].path().join("removed-before-blocker");
    fs::write(&removed_first, b"removed first").unwrap();
    let blocked_dir = entries[1].path().join("readonly-remainder");
    fs::create_dir(&blocked_dir).unwrap();
    fs::write(blocked_dir.join("still-present"), b"retry me").unwrap();
    fs::set_permissions(&blocked_dir, fs::Permissions::from_mode(0o500)).unwrap();

    let first = burn_session_home(&session.root, &expected_burn(&session, &user_root));
    match first {
        Ok(true) => {
            // 修复后的销毁器会先授权已验证子树，因而不会进入重试分支。
            assert!(!session.root.exists());
        }
        Ok(false) => panic!("existing session unexpectedly vanished before burn"),
        Err(first_error) => {
            let still_same_root = fs::symlink_metadata(&session.root)
                .map(|stats| stats.ino() == session.ino && stats.dev() == session.dev)
                .unwrap_or(false);
            let identity_is_retryable =
                assert_burn_identity(&session.root, &expected_burn(&session, &user_root)).is_ok();
            let partial_removal_happened = !removed_first.exists();
            fs::set_permissions(&blocked_dir, fs::Permissions::from_mode(0o700)).unwrap();
            let retry = burn_session_home(&session.root, &expected_burn(&session, &user_root));

            assert!(
                still_same_root,
                "failed burn changed root identity: {first_error}"
            );
            assert!(identity_is_retryable, "failed burn lost session identity");
            assert!(
                partial_removal_happened,
                "fixture did not reach a partial delete"
            );
            assert!(retry.unwrap(), "retry must burn the same session");
        }
    }
    fs::remove_dir_all(root).unwrap();
}
