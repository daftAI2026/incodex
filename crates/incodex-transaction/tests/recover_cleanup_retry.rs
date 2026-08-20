use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_transaction::{journal_v2, recover_with, Engine};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let sequence = SEQ.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("incodex-recover-cleanup-{now}-{sequence}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn app(root: &Path, name: &str, marker: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("marker"), marker).unwrap();
    path
}

#[test]
fn recover_retries_cleanup_after_restore_intent_is_durable() {
    let root = scratch();
    let target = app(&root, "ChatGPT.app", "original");
    let candidate = app(&root, "candidate.app", "patched");
    let mut tx = Engine::begin(&root, &target, "recover-cleanup-test").unwrap();
    let id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    fs::create_dir_all(&original).unwrap();
    fs::copy(target.join("marker"), original.join("marker")).unwrap();
    tx.mark_backup_committed().unwrap();
    tx.place_staging(&candidate).unwrap();
    tx.swap().unwrap();
    drop(tx);

    let trash = root
        .join("transactions")
        .join(&id)
        .join("trash/ChatGPT.app");
    let first = recover_with(&root, &id, |_| {
        fs::create_dir_all(&trash).unwrap();
        fs::write(trash.join("leftover"), "retry-me").unwrap();
        false
    });
    assert!(
        first.is_err(),
        "verification failure should leave recovery pending"
    );
    assert_eq!(journal_v2(&root, &id).unwrap().phase, "SWAPPED");
    assert!(trash.exists());

    let second = recover_with(&root, &id, |_| true).unwrap();

    assert_eq!(second.journal.phase, "ROLLED_BACK");
    assert!(!trash.exists(), "already-restored recovery skipped cleanup");
    assert_eq!(
        fs::read_to_string(target.join("marker")).unwrap(),
        "original"
    );
}
