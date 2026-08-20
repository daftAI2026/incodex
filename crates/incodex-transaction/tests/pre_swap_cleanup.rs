use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_transaction::{journal_v2, recover, Engine};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let sequence = SEQ.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("incodex-pre-swap-cleanup-{now}-{sequence}"));
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
fn pre_swap_recovery_cleans_a_leaf_symlink_without_touching_its_target() {
    let root = scratch();
    let target = app(&root, "ChatGPT.app", "healthy");
    let victim = app(&root, "victim.app", "do-not-delete");
    let tx = Engine::begin(&root, &target, "pre-swap-cleanup-test").unwrap();
    let staged = tx.staging_app();
    fs::create_dir_all(staged.parent().unwrap()).unwrap();
    symlink(&victim, &staged).unwrap();
    let id = tx.install_id().to_string();
    drop(tx);

    recover(&root, &id).unwrap();

    assert_eq!(fs::read_to_string(target.join("marker")).unwrap(), "healthy");
    assert!(!staged.exists(), "pre-swap staging symlink was not cleaned");
    assert_eq!(fs::read_to_string(victim.join("marker")).unwrap(), "do-not-delete");
    assert_eq!(journal_v2(&root, &id).unwrap().phase, "ROLLED_BACK");
}
