use std::fs;
use std::path::{Path, PathBuf};

use super::Engine;
use crate::durable::{reset_sync_trace, sync_trace};
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
        digest_call_count() <= 8,
        "adjacent phases rescanned immutable trees: {} calls",
        digest_call_count()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn backup_tree_and_ancestry_are_flushed_before_backup_commits() {
    let root = std::env::temp_dir().join(format!("incodex-backup-sync-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let target = app(&root, "ChatGPT.app", "original");
    let mut tx = Engine::begin(&root, &target, "backup-sync-test").unwrap();
    let transaction = root.join("transactions").join(tx.install_id());
    let original_parent = transaction.join("original");
    let original = original_parent.join("ChatGPT.app");
    let marker = original.join("marker");
    fs::create_dir_all(&original).unwrap();
    fs::copy(target.join("marker"), &marker).unwrap();

    reset_sync_trace();
    tx.mark_backup_committed().unwrap();

    let trace = sync_trace();
    let marker_index = trace.iter().position(|path| path == &marker).unwrap();
    let app_index = trace.iter().position(|path| path == &original).unwrap();
    let original_index = trace
        .iter()
        .position(|path| path == &original_parent)
        .unwrap();
    let transaction_index = trace.iter().position(|path| path == &transaction).unwrap();
    assert!(marker_index < app_index);
    assert!(app_index < original_index);
    assert!(original_index < transaction_index);
    assert_eq!(tx.journal().phase, "BACKUP_COMMITTED");
    fs::remove_dir_all(root).unwrap();
}
