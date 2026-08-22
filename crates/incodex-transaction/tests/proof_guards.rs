use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_transaction::Engine;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let sequence = SEQ.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("incodex-proof-guards-{now}-{sequence}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn app(root: &Path, name: &str, marker: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(path.join("Contents/Resources")).unwrap();
    fs::write(path.join("marker"), marker).unwrap();
    fs::write(path.join("Contents/Resources/config"), "config").unwrap();
    path
}

fn original_path(root: &Path, tx: &Engine) -> PathBuf {
    root.join("transactions")
        .join(tx.install_id())
        .join("original/ChatGPT.app")
}

fn copy_complete_snapshot(root: &Path, tx: &Engine, target: &Path) {
    let original = original_path(root, tx);
    fs::create_dir_all(original.join("Contents/Resources")).unwrap();
    fs::copy(target.join("marker"), original.join("marker")).unwrap();
    fs::copy(
        target.join("Contents/Resources/config"),
        original.join("Contents/Resources/config"),
    )
    .unwrap();
}

#[test]
fn backup_seal_rejects_live_content_changed_after_begin() {
    let root = scratch();
    let target = app(&root, "ChatGPT.app", "live");
    let mut tx = Engine::begin(&root, &target, "proof-guard-test").unwrap();
    copy_complete_snapshot(&root, &tx, &target);
    fs::write(target.join("marker"), "changed-after-begin").unwrap();

    let result = tx.mark_backup_committed();

    assert!(
        result.is_err(),
        "backup sealed a target changed after begin"
    );
    assert_eq!(tx.journal().phase, "DISCOVERED");
}

#[test]
fn backup_seal_rejects_a_partial_snapshot() {
    let root = scratch();
    let target = app(&root, "ChatGPT.app", "live");
    let mut tx = Engine::begin(&root, &target, "proof-guard-test").unwrap();
    let original = original_path(&root, &tx);
    fs::create_dir_all(&original).unwrap();
    fs::copy(target.join("marker"), original.join("marker")).unwrap();

    let result = tx.mark_backup_committed();

    assert!(result.is_err(), "partial backup was sealed as complete");
    assert_eq!(tx.journal().phase, "DISCOVERED");
}

#[test]
fn swap_rejects_staged_content_changed_after_place() {
    let root = scratch();
    let target = app(&root, "ChatGPT.app", "live");
    let candidate = app(&root, "candidate.app", "patched");
    let mut tx = Engine::begin(&root, &target, "proof-guard-test").unwrap();
    copy_complete_snapshot(&root, &tx, &target);
    tx.mark_backup_committed().unwrap();
    tx.place_staging(&candidate).unwrap();
    fs::write(tx.staging_app().join("marker"), "mutated-staging").unwrap();

    let result = tx.swap();

    assert!(
        result.is_err(),
        "swap accepted a staging tree changed after place"
    );
    assert_eq!(tx.journal().phase, "STAGED");
    assert_eq!(fs::read_to_string(target.join("marker")).unwrap(), "live");
}

#[test]
fn swap_rejects_staged_permissions_changed_after_place() {
    let root = scratch();
    let target = app(&root, "ChatGPT.app", "live");
    let candidate = app(&root, "candidate.app", "patched");
    let mut tx = Engine::begin(&root, &target, "proof-guard-test").unwrap();
    copy_complete_snapshot(&root, &tx, &target);
    tx.mark_backup_committed().unwrap();
    tx.place_staging(&candidate).unwrap();
    fs::set_permissions(
        tx.staging_app().join("marker"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();

    let result = tx.swap();

    assert!(
        result.is_err(),
        "swap accepted a staging mode changed after place"
    );
    assert_eq!(tx.journal().phase, "STAGED");
    assert_eq!(fs::read_to_string(target.join("marker")).unwrap(), "live");
}
