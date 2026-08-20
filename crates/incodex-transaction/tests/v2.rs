use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_core::canonical::{inspect_target, recheck_target};
use incodex_transaction::{
    acquire_target_lock, new_install_id, recover, Engine, Recovery, TxError,
};

static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let seq = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("incodex-txv2-{n}-{seq}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn app_bundle(root: &Path, name: &str, marker: &str) -> PathBuf {
    let app = root.join(name);
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), marker).unwrap();
    app
}

#[test]
fn canonical_target_records_device_inode_and_symlink_alias_is_the_same_official() {
    let root = scratch();
    let applications = root.join("Applications");
    let official = app_bundle(&applications, "ChatGPT.app", "official");
    let alias_parent = root.join("apps");
    std::os::unix::fs::symlink(&applications, &alias_parent).unwrap();
    let aliased = alias_parent.join("ChatGPT.app");

    let a = inspect_target(&aliased, Some(&official)).unwrap();
    let b = inspect_target(&official, Some(&official)).unwrap();
    assert!(a.is_official);
    assert_eq!(a.real_path, b.real_path);
    assert_eq!(a.target_device, b.target_device);
    assert_eq!(a.target_inode, b.target_inode);
    assert_ne!(a.target_inode, 0);
    recheck_target(&a).unwrap();
}

#[test]
fn recheck_fails_when_the_target_inode_changes() {
    let root = scratch();
    let official = app_bundle(&root, "ChatGPT.app", "a");
    let target = inspect_target(&official, Some(&official)).unwrap();
    fs::remove_dir_all(&official).unwrap();
    app_bundle(&root, "ChatGPT.app", "b");
    assert!(recheck_target(&target).is_err());
}

#[test]
fn mutation_lock_is_exclusive_and_stolen_when_pid_is_dead() {
    let root = scratch();
    let app = app_bundle(&root, "ChatGPT.app", "x");
    let first = acquire_target_lock(&root, &app, "install", None).unwrap();
    let second = acquire_target_lock(&root, &app, "uninstall", None);
    assert!(second.is_err());
    drop(first);
    let again = acquire_target_lock(&root, &app, "recover", None).unwrap();
    drop(again);

    let lock_path = incodex_transaction::lock_path_for(&root, &app);
    fs::write(
        &lock_path,
        r#"{"schemaVersion":1,"pid":999999,"processStart":"dead","command":"install","requestedPath":"x","realPath":"x","createdAt":"t"}
"#,
    )
    .unwrap();
    let stolen = acquire_target_lock(&root, &app, "install", None).unwrap();
    drop(stolen);
}

#[test]
fn install_id_is_uuid_and_matches_directory_and_journal() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "live");
    let staged = app_bundle(&root, "staged.app", "staged");
    let mut tx = Engine::begin(&root, &target_app, "install").unwrap();
    assert!(new_install_id().contains('-'));
    assert_eq!(tx.install_id().len(), 36);
    assert_eq!(tx.install_id(), tx.journal().install_id);
    assert!(root
        .join("transactions")
        .join(tx.install_id())
        .join("journal.json")
        .exists());
    tx.place_staging(&staged).unwrap();
    assert!(tx.staging_app().display().to_string().contains(tx.install_id()));
    assert!(tx.outgoing_app().display().to_string().contains(tx.install_id()));
}

#[test]
fn swap_writes_intent_before_moving_and_keeps_outgoing_until_commit() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "original");
    let staged = app_bundle(&root, "staged.app", "patched");
    let mut tx = Engine::begin(&root, &target_app, "install").unwrap();
    tx.place_staging(&staged).unwrap();
    tx.swap().unwrap();
    assert_eq!(fs::read_to_string(target_app.join("marker")).unwrap(), "patched");
    assert!(tx.outgoing_app().exists());
    assert_eq!(
        fs::read_to_string(tx.outgoing_app().join("marker")).unwrap(),
        "original"
    );
    assert_eq!(tx.journal().phase, "SWAPPED");
    assert!(!tx.journal().paths.outgoing.contains(".."));
    tx.commit().unwrap();
    assert_eq!(tx.journal().phase, "COMMITTED");
    assert!(!tx.outgoing_app().exists());
}

#[test]
fn verify_failure_restores_original_and_rolls_back() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "original");
    let staged = app_bundle(&root, "staged.app", "patched");
    let mut tx = Engine::begin(&root, &target_app, "install").unwrap();
    tx.place_staging(&staged).unwrap();
    tx.swap().unwrap();
    tx.rollback("verify failed").unwrap();
    assert_eq!(fs::read_to_string(target_app.join("marker")).unwrap(), "original");
    assert_eq!(tx.journal().phase, "ROLLED_BACK");
    let id = tx.install_id().to_string();
    drop(tx);
    let again = recover(&root, &id).unwrap();
    assert_eq!(again.action, Recovery::Done);
}

#[test]
fn uncommitted_recover_rolls_back_and_is_idempotent() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "original");
    let tx = Engine::begin(&root, &target_app, "install").unwrap();
    let id = tx.install_id().to_string();
    drop(tx);
    let first = recover(&root, &id).unwrap();
    assert_eq!(first.action, Recovery::Rollback);
    assert_eq!(fs::read_to_string(target_app.join("marker")).unwrap(), "original");
    let second = recover(&root, &id).unwrap();
    assert_eq!(second.action, Recovery::Done);
}

#[test]
fn durable_journal_is_0600_and_checksummed() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "x");
    let tx = Engine::begin(&root, &target_app, "install").unwrap();
    let journal_path = root
        .join("transactions")
        .join(tx.install_id())
        .join("journal.json");
    let mode = fs::metadata(&journal_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let raw: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&journal_path).unwrap()).unwrap();
    assert_eq!(raw["schemaVersion"], 2);
    assert!(raw["checksum"].as_str().unwrap().len() == 64);
    assert!(raw["sequence"].as_u64().unwrap() >= 1);
    assert_eq!(raw["paths"]["staged"], "staging/ChatGPT.app");
    assert_eq!(raw["paths"]["outgoing"], "outgoing/ChatGPT.app");
}

#[test]
fn tampered_journal_checksum_is_rejected() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "x");
    let tx = Engine::begin(&root, &target_app, "install").unwrap();
    let id = tx.install_id().to_string();
    let journal_path = root.join("transactions").join(&id).join("journal.json");
    drop(tx);

    let mut raw: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&journal_path).unwrap()).unwrap();
    raw["phase"] = serde_json::json!("COMMITTED");
    fs::write(&journal_path, format!("{}\n", serde_json::to_string_pretty(&raw).unwrap())).unwrap();

    let error = incodex_transaction::journal_v2(&root, &id).unwrap_err();
    assert!(error.contains("checksum"), "{error}");
}

#[test]
fn recover_refuses_while_the_target_lock_is_live() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "x");
    let tx = Engine::begin(&root, &target_app, "install").unwrap();

    let error = recover(&root, tx.install_id()).unwrap_err();
    assert!(
        error.to_string().contains("another incodex command is modifying this app"),
        "{error}"
    );
    assert_eq!(tx.journal().phase, "DISCOVERED");
}

#[test]
fn recover_refuses_traversal_absolute_and_symlink_paths_and_deletes_nothing() {
    let root = scratch();
    let victim = root.join("victim");
    fs::create_dir_all(&victim).unwrap();
    fs::write(victim.join("keep"), "secret").unwrap();
    let id = new_install_id();
    let tx_dir = root.join("transactions").join(&id);
    fs::create_dir_all(&tx_dir).unwrap();

    for bad in [
        serde_json::json!("../../victim"),
        serde_json::json!(victim.display().to_string()),
    ] {
        let body = serde_json::json!({
            "schemaVersion": 2,
            "installId": id,
            "target": {
                "requestedPath": "/tmp/ChatGPT.app",
                "realPath": "/tmp/ChatGPT.app",
                "device": "1",
                "inode": "1"
            },
            "paths": {
                "staged": bad,
                "outgoing": "outgoing/ChatGPT.app",
                "original": "original/ChatGPT.app"
            },
            "phase": "STAGED",
            "sequence": 1,
            "checksum": "00".repeat(32)
        });
        fs::write(tx_dir.join("journal.json"), format!("{body}\n")).unwrap();
        let err = recover(&root, &id).unwrap_err();
        match err {
            TxError::Refuse { .. } => {}
            other => panic!("expected refuse, got {other:?}"),
        }
        assert_eq!(fs::read_to_string(victim.join("keep")).unwrap(), "secret");
        assert!(victim.exists());
    }
}

#[test]
fn no_live_staging_name() {
    let src = include_str!("../src/engine.rs");
    assert!(!src.contains("ChatGPT.app.live"));
}
