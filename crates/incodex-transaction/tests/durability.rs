use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_transaction::Engine;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("incodex-durability-{now}-{seq}"));
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
fn swap_checkpoints_are_after_parent_directory_durability() {
    let root = scratch();
    let target = app(&root, "ChatGPT.app", "original");
    let candidate = app(&root, "candidate.app", "patched");
    let mut tx = Engine::begin(&root, &target, "durability-test").unwrap();
    let original = root
        .join("transactions")
        .join(tx.install_id())
        .join("original/ChatGPT.app");
    fs::create_dir_all(&original).unwrap();
    fs::copy(target.join("marker"), original.join("marker")).unwrap();
    tx.mark_backup_committed().unwrap();
    tx.place_staging(&candidate).unwrap();

    let mut checkpoints = Vec::new();
    tx.swap_with_checkpoint(|phase| checkpoints.push(phase.to_string()))
        .unwrap();
    assert_eq!(
        checkpoints,
        [
            "TARGET_MOVED_OUT",
            "LIVE_MOVED_OUT_DURABLE",
            "LIVE_MOVED_OUT",
            "STAGING_MOVED_IN_DURABLE",
            "STAGING_MOVED_IN",
            "SWAPPED",
        ]
    );
}
