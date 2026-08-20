use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_transaction::{journal_v2, recover_with, Engine};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("incodex-recover-identity-{now}-{n}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn app(root: &Path, name: &str, marker: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("marker"), marker).unwrap();
    path
}

fn interrupted(phase: &str) -> (PathBuf, PathBuf, String) {
    let root = scratch();
    let target = app(&root, "ChatGPT.app", "original");
    let candidate = app(&root, "candidate.app", "patched");
    let mut tx = Engine::begin(&root, &target, "recover-identity-test").unwrap();
    let id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    fs::create_dir_all(&original).unwrap();
    fs::copy(target.join("marker"), original.join("marker")).unwrap();
    tx.mark_backup_committed().unwrap();
    if phase == "SWAPPED" {
        tx.place_staging(&candidate).unwrap();
        tx.swap().unwrap();
    }
    drop(tx);
    (root, target, id)
}

#[test]
fn recover_refuses_to_overwrite_a_replaced_live_after_swap() {
    let (root, target, id) = interrupted("SWAPPED");
    let moved = root.join("later-live.app");
    fs::rename(&target, &moved).unwrap();
    app(&root, "ChatGPT.app", "later-upgrade");

    let result = recover_with(&root, &id, |_| true);

    assert!(result.is_err(), "old transaction overwrote a later live app");
    assert_eq!(fs::read_to_string(target.join("marker")).unwrap(), "later-upgrade");
    assert_eq!(journal_v2(&root, &id).unwrap().phase, "SWAPPED");
}

#[test]
fn recover_refuses_to_finish_a_pre_swap_transaction_after_live_replacement() {
    let (root, target, id) = interrupted("DISCOVERED");
    let moved = root.join("later-live.app");
    fs::rename(&target, &moved).unwrap();
    app(&root, "ChatGPT.app", "later-upgrade");

    let result = recover_with(&root, &id, |_| true);

    assert!(result.is_err(), "stale pre-swap transaction was accepted");
    assert_eq!(fs::read_to_string(target.join("marker")).unwrap(), "later-upgrade");
    assert_eq!(journal_v2(&root, &id).unwrap().phase, "BACKUP_COMMITTED");
}

#[test]
fn recover_accepts_the_inode_change_caused_by_its_own_swap() {
    let (root, target, id) = interrupted("SWAPPED");

    let result = recover_with(&root, &id, |_| true).unwrap();

    assert!(result.action.as_str() == "rollback");
    assert_eq!(fs::read_to_string(target.join("marker")).unwrap(), "original");
    assert_eq!(journal_v2(&root, &id).unwrap().phase, "ROLLED_BACK");
}
