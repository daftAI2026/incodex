use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use incodex_transaction::{
    recover_with_quiescence, restore_committed_with_quiescence, journal_v2, Engine,
};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "incodex-transaction-quiescence-{}-{n}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn app(root: &Path) -> PathBuf {
    let app = root.join("ChatGPT.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), b"original").unwrap();
    app
}

fn candidate(root: &Path) -> PathBuf {
    let app = root.join("candidate.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), b"patched").unwrap();
    app
}

fn backup(root: &Path, tx: &Engine) -> PathBuf {
    let path = root
        .join("transactions")
        .join(tx.install_id())
        .join("original/ChatGPT.app");
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("marker"), b"original").unwrap();
    path
}

fn allow_calls(count: usize) -> impl Fn(&Path) -> Result<(), String> {
    let calls = Arc::new(AtomicU64::new(0));
    move |_target| {
        let call = calls.fetch_add(1, Ordering::Relaxed) as usize;
        if call < count {
            Ok(())
        } else {
            Err("fixture app became live at a destructive boundary".into())
        }
    }
}

fn refuse_quiescence(_target: &Path) -> Result<(), String> {
    Err("fixture app is still running".into())
}

fn committed_transaction(root: &Path, target: &Path) -> Engine {
    let mut tx = Engine::begin_with_quiescence(root, target, "test", allow_calls(16)).unwrap();
    backup(root, &tx);
    tx.mark_backup_committed().unwrap();
    let staged = candidate(root);
    tx.place_staging(&staged).unwrap();
    tx.swap().unwrap();
    tx.commit().unwrap();
    tx
}

#[test]
fn begin_quiescence_refusal_happens_after_lock_before_digest_or_journal() {
    let root = scratch();
    let target = app(&root);
    let before = fs::read(target.join("marker")).unwrap();
    let error = Engine::begin_with_quiescence(&root, &target, "test", refuse_quiescence)
    .err()
    .unwrap();

    assert!(error.contains("fixture app is still running"), "{error}");
    assert_eq!(fs::read(target.join("marker")).unwrap(), before);
    assert!(
        !root.join("transactions").exists()
            || fs::read_dir(root.join("transactions")).unwrap().next().is_none(),
        "quiescence refusal must not create a journal"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn backup_seal_quiescence_refusal_keeps_target_and_journal() {
    let root = scratch();
    let target = app(&root);
    let mut tx = Engine::begin_with_quiescence(&root, &target, "test", allow_calls(1)).unwrap();
    let install_id = tx.install_id().to_string();
    let original = backup(&root, &tx);

    let error = tx.mark_backup_committed().unwrap_err();

    assert!(error.contains("destructive boundary"), "{error}");
    assert_eq!(fs::read(target.join("marker")).unwrap(), b"original");
    assert_eq!(journal_v2(&root, &install_id).unwrap().phase, "DISCOVERED");
    assert!(original.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn swap_quiescence_refusal_keeps_live_target_and_staged_phase() {
    let root = scratch();
    let target = app(&root);
    let mut tx = Engine::begin_with_quiescence(&root, &target, "test", allow_calls(3)).unwrap();
    let install_id = tx.install_id().to_string();
    backup(&root, &tx);
    tx.mark_backup_committed().unwrap();
    let staged = candidate(&root);
    tx.place_staging(&staged).unwrap();

    let error = tx.swap().unwrap_err();

    assert!(error.contains("destructive boundary"), "{error}");
    assert_eq!(fs::read(target.join("marker")).unwrap(), b"original");
    assert_eq!(journal_v2(&root, &install_id).unwrap().phase, "STAGED");
    assert!(tx.staging_app().exists());
    assert!(!tx.outgoing_app().exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn committed_restore_quiescence_refusal_does_not_advance_journal_or_target() {
    let root = scratch();
    let target = app(&root);
    let tx = committed_transaction(&root, &target);
    let install_id = tx.install_id().to_string();
    drop(tx);
    let before = fs::read(target.join("marker")).unwrap();

    let error = restore_committed_with_quiescence(
        &root,
        &install_id,
        &target,
        refuse_quiescence,
        |_| {},
    )
    .unwrap_err();

    assert!(error.contains("still running"), "{error}");
    assert_eq!(fs::read(target.join("marker")).unwrap(), before);
    assert_eq!(journal_v2(&root, &install_id).unwrap().phase, "COMMITTED");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn recover_restore_quiescence_refusal_keeps_swapped_truth() {
    let root = scratch();
    let target = app(&root);
    let mut tx = Engine::begin_with_quiescence(&root, &target, "test", allow_calls(4)).unwrap();
    let install_id = tx.install_id().to_string();
    backup(&root, &tx);
    tx.mark_backup_committed().unwrap();
    let staged = candidate(&root);
    tx.place_staging(&staged).unwrap();
    tx.swap().unwrap();
    drop(tx);
    let before = fs::read(target.join("marker")).unwrap();

    let error = recover_with_quiescence(
        &root,
        &install_id,
        refuse_quiescence,
        |_| true,
    )
    .unwrap_err();

    assert!(error.to_string().contains("still running"), "{error}");
    assert_eq!(fs::read(target.join("marker")).unwrap(), before);
    assert_eq!(journal_v2(&root, &install_id).unwrap().phase, "SWAPPED");
    fs::remove_dir_all(root).unwrap();
}
