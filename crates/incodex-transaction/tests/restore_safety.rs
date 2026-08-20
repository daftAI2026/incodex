use std::fs;
use std::os::unix::fs::symlink;
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
    let root = std::env::temp_dir().join(format!("incodex-restore-safety-{now}-{n}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn app(root: &Path, name: &str, marker: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("marker"), marker).unwrap();
    path
}

struct Interrupted {
    root: PathBuf,
    target: PathBuf,
    id: String,
    original: PathBuf,
    outgoing: PathBuf,
}

fn interrupted() -> Interrupted {
    let root = scratch();
    let target = app(&root, "ChatGPT.app", "patched");
    let candidate = app(&root, "candidate.app", "candidate");
    let mut tx = Engine::begin(&root, &target, "restore-safety-test").unwrap();
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
    let outgoing = tx.outgoing_app();
    drop(tx);
    Interrupted {
        root,
        target,
        id,
        original,
        outgoing,
    }
}

#[test]
fn recover_refuses_when_no_restore_source_exists() {
    let interrupted = interrupted();
    fs::remove_dir_all(&interrupted.original).unwrap();
    fs::remove_dir_all(&interrupted.outgoing).unwrap();

    let result = recover_with(&interrupted.root, &interrupted.id, |_| true);

    assert!(result.is_err(), "missing restore source was accepted");
    assert_eq!(
        journal_v2(&interrupted.root, &interrupted.id)
            .unwrap()
            .phase,
        "SWAPPED"
    );
    assert_eq!(
        fs::read_to_string(interrupted.target.join("marker")).unwrap(),
        "candidate"
    );
}

#[test]
fn recover_refuses_file_or_symlink_original_without_falling_back() {
    for kind in ["file", "symlink"] {
        let interrupted = interrupted();
        fs::remove_dir_all(&interrupted.original).unwrap();
        if kind == "file" {
            fs::write(&interrupted.original, b"partial").unwrap();
        } else {
            let victim = interrupted.root.join("victim");
            fs::create_dir_all(&victim).unwrap();
            fs::write(victim.join("marker"), "victim").unwrap();
            symlink(&victim, &interrupted.original).unwrap();
        }

        let result = recover_with(&interrupted.root, &interrupted.id, |_| true);

        assert!(result.is_err(), "{kind} restore source was accepted");
        assert_eq!(
            journal_v2(&interrupted.root, &interrupted.id)
                .unwrap()
                .phase,
            "SWAPPED"
        );
        assert_eq!(
            fs::read_to_string(interrupted.target.join("marker")).unwrap(),
            "candidate"
        );
    }
}

#[test]
fn recover_refuses_a_modified_sealed_original() {
    let interrupted = interrupted();
    fs::write(interrupted.original.join("marker"), "partial").unwrap();

    let result = recover_with(&interrupted.root, &interrupted.id, |_| true);

    assert!(result.is_err(), "modified backup was accepted");
    assert_eq!(
        journal_v2(&interrupted.root, &interrupted.id)
            .unwrap()
            .phase,
        "SWAPPED"
    );
    assert_eq!(
        fs::read_to_string(interrupted.target.join("marker")).unwrap(),
        "candidate"
    );
}

#[test]
fn recover_refuses_a_partial_outgoing_when_original_is_missing() {
    let interrupted = interrupted();
    fs::remove_dir_all(&interrupted.original).unwrap();
    fs::write(interrupted.outgoing.join("marker"), "partial").unwrap();

    let result = recover_with(&interrupted.root, &interrupted.id, |_| true);

    assert!(result.is_err(), "partial outgoing was accepted");
    assert_eq!(
        journal_v2(&interrupted.root, &interrupted.id)
            .unwrap()
            .phase,
        "SWAPPED"
    );
    assert_eq!(
        fs::read_to_string(interrupted.target.join("marker")).unwrap(),
        "candidate"
    );
}
