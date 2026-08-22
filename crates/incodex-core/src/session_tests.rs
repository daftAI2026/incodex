
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
