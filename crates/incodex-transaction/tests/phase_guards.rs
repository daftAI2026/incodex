use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_transaction::Engine;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("incodex-phase-guards-{now}-{n}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn app(root: &Path, name: &str, marker: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("marker"), marker).unwrap();
    path
}

fn seal_backup(root: &Path, tx: &mut Engine, target: &Path) {
    let original = root
        .join("transactions")
        .join(tx.install_id())
        .join("original/ChatGPT.app");
    fs::create_dir_all(&original).unwrap();
    fs::copy(target.join("marker"), original.join("marker")).unwrap();
    tx.mark_backup_committed().unwrap();
}

fn swapped_transaction() -> (PathBuf, PathBuf, Engine) {
    let root = scratch();
    let target = app(&root, "ChatGPT.app", "original");
    let candidate = app(&root, "candidate.app", "patched");
    let mut tx = Engine::begin(&root, &target, "phase-guard-test").unwrap();
    seal_backup(&root, &mut tx, &target);
    tx.place_staging(&candidate).unwrap();
    tx.swap().unwrap();
    (root, target, tx)
}

#[test]
fn swap_requires_the_staged_phase() {
    let root = scratch();
    let target = app(&root, "ChatGPT.app", "original");
    let mut tx = Engine::begin(&root, &target, "phase-guard-test").unwrap();

    let result = tx.swap();

    assert!(result.is_err(), "swap escaped from DISCOVERED");
    assert_eq!(tx.journal().phase, "DISCOVERED");
    assert_eq!(fs::read_to_string(target.join("marker")).unwrap(), "original");
    assert!(!tx.outgoing_app().exists());
}

#[test]
fn commit_requires_the_swapped_phase() {
    let root = scratch();
    let target = app(&root, "ChatGPT.app", "original");
    let candidate = app(&root, "candidate.app", "patched");
    let mut tx = Engine::begin(&root, &target, "phase-guard-test").unwrap();
    seal_backup(&root, &mut tx, &target);
    tx.place_staging(&candidate).unwrap();

    let result = tx.commit();

    assert!(result.is_err(), "commit escaped from STAGED");
    assert_eq!(tx.journal().phase, "STAGED");
    assert_eq!(fs::read_to_string(target.join("marker")).unwrap(), "original");
}

#[test]
fn committed_transaction_rejects_a_second_commit() {
    let (_root, _target, mut tx) = swapped_transaction();
    tx.commit().unwrap();

    let result = tx.commit();

    assert!(result.is_err(), "second commit was accepted");
    assert_eq!(tx.journal().phase, "COMMITTED");
}

#[test]
fn rolled_back_transaction_rejects_a_second_rollback() {
    let root = scratch();
    let target = app(&root, "ChatGPT.app", "original");
    let mut tx = Engine::begin(&root, &target, "phase-guard-test").unwrap();
    seal_backup(&root, &mut tx, &target);
    tx.rollback("test").unwrap();

    let result = tx.rollback("repeat");

    assert!(result.is_err(), "second rollback was accepted");
    assert_eq!(tx.journal().phase, "ROLLED_BACK");
    assert_eq!(fs::read_to_string(target.join("marker")).unwrap(), "original");
}
