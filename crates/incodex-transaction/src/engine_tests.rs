use std::fs;
use std::path::{Path, PathBuf};

use super::Engine;
use crate::proof::{digest_call_count, reset_digest_call_count};

fn app(root: &Path, name: &str, marker: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("marker"), marker).unwrap();
    path
}

#[test]
fn install_phases_bound_repeated_tree_digest_scans() {
    let root = std::env::temp_dir().join(format!("incodex-digest-budget-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let target = app(&root, "ChatGPT.app", "original");
    let candidate = app(&root, "candidate.app", "patched");
    let mut tx = Engine::begin(&root, &target, "digest-budget-test").unwrap();
    let original = root
        .join("transactions")
        .join(tx.install_id())
        .join("original/ChatGPT.app");
    fs::create_dir_all(&original).unwrap();
    fs::copy(target.join("marker"), original.join("marker")).unwrap();

    reset_digest_call_count();
    tx.mark_backup_committed().unwrap();
    tx.place_staging(&candidate).unwrap();
    tx.swap().unwrap();
    tx.commit().unwrap();

    assert!(
        digest_call_count() <= 7,
        "adjacent phases rescanned immutable trees: {} calls",
        digest_call_count()
    );
    fs::remove_dir_all(root).unwrap();
}
