use std::fs;
use std::io::Write;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use incodex_core::canonical::{inspect_target, recheck_target};
use incodex_transaction::{
    acquire_target_lock, new_install_id, recover, Engine, Recovery, TxError,
};

static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let seq = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("incodex-txv2-{n}-{seq}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn app_bundle(root: &Path, name: &str, marker: &str) -> PathBuf {
    let app = root.join(name);
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), marker).unwrap();
    app
}

fn seal_backup(root: &Path, tx: &mut Engine, app: &Path) {
    let original = root
        .join("transactions")
        .join(tx.install_id())
        .join("original/ChatGPT.app");
    fs::create_dir_all(&original).unwrap();
    fs::copy(app.join("marker"), original.join("marker")).unwrap();
    tx.mark_backup_committed().unwrap();
}

#[test]
fn canonical_target_records_device_inode_and_symlink_alias_is_the_same_official() {
    let root = scratch();
    let applications = root.join("Applications");
    let official = app_bundle(&applications, "ChatGPT.app", "official");
    let alias_parent = root.join("apps");
    std::os::unix::fs::symlink(&applications, &alias_parent).unwrap();
    let aliased = alias_parent.join("ChatGPT.app");

    let a = inspect_target(&aliased, Some(&official)).unwrap();
    let b = inspect_target(&official, Some(&official)).unwrap();
    assert!(a.is_official);
    assert_eq!(a.real_path, b.real_path);
    assert_eq!(a.target_device, b.target_device);
    assert_eq!(a.target_inode, b.target_inode);
    assert_ne!(a.target_inode, 0);
    recheck_target(&a).unwrap();
}

#[test]
fn recheck_fails_when_the_target_inode_changes() {
    let root = scratch();
    let official = app_bundle(&root, "ChatGPT.app", "a");
    let target = inspect_target(&official, Some(&official)).unwrap();
    fs::remove_dir_all(&official).unwrap();
    app_bundle(&root, "ChatGPT.app", "b");
    assert!(recheck_target(&target).is_err());
}

#[test]
fn mutation_lock_is_exclusive_and_stolen_when_pid_is_dead() {
    let root = scratch();
    let app = app_bundle(&root, "ChatGPT.app", "x");
    let first = acquire_target_lock(&root, &app, "install", None).unwrap();
    let second = acquire_target_lock(&root, &app, "uninstall", None);
    assert!(second.is_err());
    drop(first);
    let again = acquire_target_lock(&root, &app, "recover", None).unwrap();
    drop(again);

    let lock_path = incodex_transaction::lock_path_for(&root, &app);
    fs::write(
        &lock_path,
        r#"{"schemaVersion":1,"pid":999999,"processStart":"dead","command":"install","requestedPath":"x","realPath":"x","createdAt":"t"}
"#,
    )
    .unwrap();
    let stolen = acquire_target_lock(&root, &app, "install", None).unwrap();
    drop(stolen);
}

#[test]
fn stale_same_pid_lock_cannot_be_removed_by_previous_owner() {
    let root = scratch();
    let app = app_bundle(&root, "ChatGPT.app", "x");
    let first = acquire_target_lock(&root, &app, "first", None).unwrap();
    let lock_path = incodex_transaction::lock_path_for(&root, &app);
    let mut record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&lock_path).unwrap()).unwrap();
    record["processStart"] = serde_json::json!("not-the-current-process");
    fs::write(
        &lock_path,
        format!("{}\n", serde_json::to_string(&record).unwrap()),
    )
    .unwrap();

    let second = acquire_target_lock(&root, &app, "second", None).unwrap();
    drop(first);
    let third = acquire_target_lock(&root, &app, "third", None);
    assert!(
        third.is_err(),
        "the previous owner removed a replacement lock from the same PID"
    );
    drop(second);
}

#[test]
fn concurrent_lock_creation_never_exposes_a_partial_record() {
    let root = scratch();
    let app = app_bundle(&root, "ChatGPT.app", "x");
    let barrier = Arc::new(Barrier::new(20));
    let release = Arc::new(AtomicBool::new(false));
    let command = Arc::new("install-".repeat(500_000));
    let (send, receive) = mpsc::channel();
    let handles: Vec<_> = (0..20)
        .map(|_| {
            let root = root.clone();
            let app = app.clone();
            let barrier = Arc::clone(&barrier);
            let release = Arc::clone(&release);
            let command = Arc::clone(&command);
            let send = send.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let lock = acquire_target_lock(&root, &app, &command, None);
                send.send(lock.is_ok()).unwrap();
                if let Ok(lock) = lock {
                    while !release.load(Ordering::Acquire) {
                        std::thread::yield_now();
                    }
                    drop(lock);
                }
            })
        })
        .collect();
    drop(send);

    let winners = (0..20).filter(|_| receive.recv().unwrap()).count();
    release.store(true, Ordering::Release);
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(winners, 1, "partial lock records allowed {winners} winners");
}

#[test]
fn twenty_processes_contending_for_one_canonical_target_have_one_winner() {
    if std::env::var_os("INCODEX_MUTATION_LOCK_WORKER").is_some() {
        return;
    }

    const PROCESS_COUNT: usize = 20;
    let root = scratch();
    let app = app_bundle(&root, "ChatGPT.app", "x");
    let coordination = root.join("coordination");
    fs::create_dir_all(&coordination).unwrap();
    let ready_dir = coordination.join("ready");
    let result_dir = coordination.join("result");
    fs::create_dir_all(&ready_dir).unwrap();
    fs::create_dir_all(&result_dir).unwrap();
    let start = coordination.join("start");
    let release = coordination.join("release");
    let executable = std::env::current_exe().unwrap();

    let mut children = Vec::with_capacity(PROCESS_COUNT);
    for id in 0..PROCESS_COUNT {
        let child = Command::new(&executable)
            .args(["--exact", "mutation_lock_process_worker", "--nocapture"])
            .env("INCODEX_MUTATION_LOCK_WORKER", "1")
            .env("INCODEX_MUTATION_LOCK_ID", id.to_string())
            .env("INCODEX_MUTATION_LOCK_ROOT", &root)
            .env("INCODEX_MUTATION_LOCK_TARGET", &app)
            .env("INCODEX_MUTATION_LOCK_READY", &ready_dir)
            .env("INCODEX_MUTATION_LOCK_RESULT", &result_dir)
            .env("INCODEX_MUTATION_LOCK_START", &start)
            .env("INCODEX_MUTATION_LOCK_RELEASE", &release)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        children.push(child);
    }

    wait_until("all lock workers ready", Duration::from_secs(10), || {
        count_files(&ready_dir) == PROCESS_COUNT
    });
    fs::write(&start, b"go\n").unwrap();

    wait_until("all lock workers reported", Duration::from_secs(10), || {
        count_files(&result_dir) == PROCESS_COUNT
    });
    let winners = fs::read_dir(&result_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .filter(|result| result == "winner\n")
        .count();
    fs::write(&release, b"release\n").unwrap();
    for child in &mut children {
        let status = child.wait().unwrap();
        assert!(status.success(), "lock worker exited with {status}");
    }
    assert_eq!(winners, 1, "twenty OS processes produced {winners} winners");
}

#[test]
fn mutation_lock_process_worker() {
    let Some(id) = std::env::var_os("INCODEX_MUTATION_LOCK_ID") else {
        return;
    };
    let root = PathBuf::from(std::env::var_os("INCODEX_MUTATION_LOCK_ROOT").unwrap());
    let target = PathBuf::from(std::env::var_os("INCODEX_MUTATION_LOCK_TARGET").unwrap());
    let ready_dir = PathBuf::from(std::env::var_os("INCODEX_MUTATION_LOCK_READY").unwrap());
    let result_dir = PathBuf::from(std::env::var_os("INCODEX_MUTATION_LOCK_RESULT").unwrap());
    let start = PathBuf::from(std::env::var_os("INCODEX_MUTATION_LOCK_START").unwrap());
    let release = PathBuf::from(std::env::var_os("INCODEX_MUTATION_LOCK_RELEASE").unwrap());

    fs::write(ready_dir.join(&id), b"ready\n").unwrap();
    wait_for_path(&start, Duration::from_secs(10));
    let lock = acquire_target_lock(&root, &target, "process-stress", None);
    let won = lock.is_ok();
    let mut result = fs::File::create(result_dir.join(&id)).unwrap();
    if won {
        result.write_all(b"winner\n").unwrap();
    } else {
        result.write_all(b"loser\n").unwrap();
    }
    result.sync_data().unwrap();
    if let Ok(lock) = lock {
        wait_for_path(&release, Duration::from_secs(10));
        drop(lock);
    }
}

fn count_files(dir: &Path) -> usize {
    fs::read_dir(dir).unwrap().filter_map(Result::ok).count()
}

fn wait_for_path(path: &Path, timeout: Duration) {
    wait_until("coordination marker", timeout, || path.exists());
}

fn wait_until(label: &str, timeout: Duration, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {label}");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn install_id_is_uuid_and_matches_directory_and_journal() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "live");
    let staged = app_bundle(&root, "staged.app", "staged");
    let mut tx = Engine::begin(&root, &target_app, "install").unwrap();
    assert!(new_install_id().contains('-'));
    assert_eq!(tx.install_id().len(), 36);
    assert_eq!(tx.install_id(), tx.journal().install_id);
    assert!(root
        .join("transactions")
        .join(tx.install_id())
        .join("journal.json")
        .exists());
    seal_backup(&root, &mut tx, &target_app);
    tx.place_staging(&staged).unwrap();
    assert!(tx
        .staging_app()
        .display()
        .to_string()
        .contains(tx.install_id()));
    assert!(tx
        .outgoing_app()
        .display()
        .to_string()
        .contains(tx.install_id()));
}

#[test]
fn backup_completion_is_durable_before_staging() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "live");
    let mut tx = Engine::begin(&root, &target_app, "install").unwrap();
    assert_eq!(tx.journal().phase, "DISCOVERED");
    let original = root
        .join("transactions")
        .join(tx.install_id())
        .join("original/ChatGPT.app");
    fs::create_dir_all(&original).unwrap();
    fs::copy(target_app.join("marker"), original.join("marker")).unwrap();
    tx.mark_backup_committed().unwrap();
    assert_eq!(tx.journal().phase, "BACKUP_COMMITTED");
    assert_eq!(
        incodex_transaction::journal_v2(&root, tx.install_id())
            .unwrap()
            .phase,
        "BACKUP_COMMITTED"
    );
}

#[test]
fn backup_completion_requires_a_real_directory_snapshot() {
    for kind in ["missing", "file", "symlink"] {
        let root = scratch();
        let target_app = app_bundle(&root, "ChatGPT.app", "live");
        let mut tx = Engine::begin(&root, &target_app, "install").unwrap();
        let original = root
            .join("transactions")
            .join(tx.install_id())
            .join("original/ChatGPT.app");
        match kind {
            "missing" => {}
            "file" => {
                fs::create_dir_all(original.parent().unwrap()).unwrap();
                fs::write(&original, b"partial").unwrap();
            }
            "symlink" => {
                fs::create_dir_all(original.parent().unwrap()).unwrap();
                symlink(&target_app, &original).unwrap();
            }
            _ => unreachable!(),
        }

        assert!(
            tx.mark_backup_committed().is_err(),
            "{kind} backup was accepted"
        );
        assert_eq!(tx.journal().phase, "DISCOVERED");
    }
}

#[test]
fn staging_requires_a_durable_backup_phase() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "live");
    let staged = app_bundle(&root, "staged.app", "staged");
    let mut tx = Engine::begin(&root, &target_app, "install").unwrap();

    let error = tx.place_staging(&staged).unwrap_err();

    assert!(error.contains("BACKUP_COMMITTED"), "{error}");
    assert_eq!(tx.journal().phase, "DISCOVERED");
    assert!(staged.exists(), "the rejected source was consumed");
    assert!(!tx.staging_app().exists());
}

#[test]
fn rollback_before_swap_never_restores_a_partial_backup_over_live() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "healthy-live");
    let mut tx = Engine::begin(&root, &target_app, "install").unwrap();
    let partial = root
        .join("transactions")
        .join(tx.install_id())
        .join("original/ChatGPT.app");
    fs::create_dir_all(&partial).unwrap();
    fs::write(partial.join("marker"), "partial-backup").unwrap();

    tx.rollback("copy failed").unwrap();

    assert_eq!(
        fs::read_to_string(target_app.join("marker")).unwrap(),
        "healthy-live"
    );
    assert!(!partial.exists(), "partial backup survived rollback");
    assert_eq!(tx.journal().phase, "ROLLED_BACK");
}

#[test]
fn committed_transaction_rejects_a_late_rollback() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "original");
    let staged = app_bundle(&root, "staged.app", "patched");
    let mut tx = Engine::begin(&root, &target_app, "install").unwrap();
    seal_backup(&root, &mut tx, &target_app);
    tx.place_staging(&staged).unwrap();
    tx.swap().unwrap();
    tx.commit().unwrap();

    assert!(tx.rollback("late failure").is_err());
    assert_eq!(
        fs::read_to_string(target_app.join("marker")).unwrap(),
        "patched"
    );
    assert_eq!(tx.journal().phase, "COMMITTED");
}

#[test]
fn swap_writes_intent_before_moving_and_keeps_outgoing_until_commit() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "original");
    let staged = app_bundle(&root, "staged.app", "patched");
    let mut tx = Engine::begin(&root, &target_app, "install").unwrap();
    seal_backup(&root, &mut tx, &target_app);
    tx.place_staging(&staged).unwrap();
    tx.swap().unwrap();
    assert_eq!(
        fs::read_to_string(target_app.join("marker")).unwrap(),
        "patched"
    );
    assert!(tx.outgoing_app().exists());
    assert_eq!(
        fs::read_to_string(tx.outgoing_app().join("marker")).unwrap(),
        "original"
    );
    assert_eq!(tx.journal().phase, "SWAPPED");
    assert!(!tx.journal().paths.outgoing.contains(".."));
    tx.commit().unwrap();
    assert_eq!(tx.journal().phase, "COMMITTED");
    assert!(!tx.outgoing_app().exists());
}

#[test]
fn verify_failure_restores_original_and_rolls_back() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "original");
    let staged = app_bundle(&root, "staged.app", "patched");
    let mut tx = Engine::begin(&root, &target_app, "install").unwrap();
    seal_backup(&root, &mut tx, &target_app);
    tx.place_staging(&staged).unwrap();
    tx.swap().unwrap();
    tx.rollback("verify failed").unwrap();
    assert_eq!(
        fs::read_to_string(target_app.join("marker")).unwrap(),
        "original"
    );
    assert_eq!(tx.journal().phase, "ROLLED_BACK");
    let id = tx.install_id().to_string();
    drop(tx);
    let again = recover(&root, &id).unwrap();
    assert_eq!(again.action, Recovery::Done);
}

#[test]
fn uncommitted_recover_rolls_back_and_is_idempotent() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "original");
    let tx = Engine::begin(&root, &target_app, "install").unwrap();
    let id = tx.install_id().to_string();
    drop(tx);
    let first = recover(&root, &id).unwrap();
    assert_eq!(first.action, Recovery::Rollback);
    assert_eq!(
        fs::read_to_string(target_app.join("marker")).unwrap(),
        "original"
    );
    let second = recover(&root, &id).unwrap();
    assert_eq!(second.action, Recovery::Done);
}

#[test]
fn durable_journal_is_0600_and_checksummed() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "x");
    let tx = Engine::begin(&root, &target_app, "install").unwrap();
    let journal_path = root
        .join("transactions")
        .join(tx.install_id())
        .join("journal.json");
    let mode = fs::metadata(&journal_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let raw: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&journal_path).unwrap()).unwrap();
    assert_eq!(raw["schemaVersion"], 2);
    assert!(raw["checksum"].as_str().unwrap().len() == 64);
    assert!(raw["sequence"].as_u64().unwrap() >= 1);
    assert_eq!(raw["paths"]["staged"], "staging/ChatGPT.app");
    assert_eq!(raw["paths"]["outgoing"], "outgoing/ChatGPT.app");
}

#[test]
fn tampered_journal_checksum_is_rejected() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "x");
    let tx = Engine::begin(&root, &target_app, "install").unwrap();
    let id = tx.install_id().to_string();
    let journal_path = root.join("transactions").join(&id).join("journal.json");
    drop(tx);

    let mut raw: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&journal_path).unwrap()).unwrap();
    raw["phase"] = serde_json::json!("COMMITTED");
    fs::write(
        &journal_path,
        format!("{}\n", serde_json::to_string_pretty(&raw).unwrap()),
    )
    .unwrap();

    let error = incodex_transaction::journal_v2(&root, &id).unwrap_err();
    assert!(error.contains("checksum"), "{error}");
}

#[test]
fn recover_refuses_while_the_target_lock_is_live() {
    let root = scratch();
    let target_app = app_bundle(&root, "ChatGPT.app", "x");
    let tx = Engine::begin(&root, &target_app, "install").unwrap();

    let error = recover(&root, tx.install_id()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("another incodex command is modifying this app"),
        "{error}"
    );
    assert_eq!(tx.journal().phase, "DISCOVERED");
}

#[test]
fn recover_refuses_traversal_absolute_and_symlink_paths_and_deletes_nothing() {
    let root = scratch();
    let victim = root.join("victim");
    fs::create_dir_all(&victim).unwrap();
    fs::write(victim.join("keep"), "secret").unwrap();
    let id = new_install_id();
    let tx_dir = root.join("transactions").join(&id);
    fs::create_dir_all(&tx_dir).unwrap();

    for bad in [
        serde_json::json!("../../victim"),
        serde_json::json!(victim.display().to_string()),
    ] {
        let body = serde_json::json!({
            "schemaVersion": 2,
            "installId": id,
            "target": {
                "requestedPath": "/tmp/ChatGPT.app",
                "realPath": "/tmp/ChatGPT.app",
                "device": "1",
                "inode": "1"
            },
            "paths": {
                "staged": bad,
                "outgoing": "outgoing/ChatGPT.app",
                "original": "original/ChatGPT.app"
            },
            "phase": "STAGED",
            "sequence": 1,
            "checksum": "00".repeat(32)
        });
        fs::write(tx_dir.join("journal.json"), format!("{body}\n")).unwrap();
        let err = recover(&root, &id).unwrap_err();
        match err {
            TxError::Refuse { .. } => {}
            other => panic!("expected refuse, got {other:?}"),
        }
        assert_eq!(fs::read_to_string(victim.join("keep")).unwrap(), "secret");
        assert!(victim.exists());
    }
}

#[test]
fn recover_refuses_ancestor_symlinks_even_when_leaf_is_missing_and_deletes_nothing() {
    for ancestor in ["staging", "outgoing", "original"] {
        let root = scratch();
        let target_app = app_bundle(&root, "ChatGPT.app", "original");
        let tx = Engine::begin(&root, &target_app, "install").unwrap();
        let id = tx.install_id().to_string();
        let tx_dir = root.join("transactions").join(&id);
        drop(tx);

        let victim = root.join(format!("victim-{ancestor}"));
        fs::create_dir_all(&victim).unwrap();
        fs::write(victim.join("keep"), "secret").unwrap();
        symlink(&victim, tx_dir.join(ancestor)).unwrap();

        let error = recover(&root, &id).unwrap_err();
        assert!(
            matches!(error, TxError::Refuse { .. }),
            "ancestor symlink {ancestor} was not refused: {error:?}"
        );
        assert_eq!(fs::read_to_string(victim.join("keep")).unwrap(), "secret");
        assert!(victim.exists());
        assert!(
            fs::symlink_metadata(tx_dir.join(ancestor))
                .unwrap()
                .file_type()
                .is_symlink(),
            "recovery replaced the {ancestor} ancestor symlink"
        );
    }
}

#[test]
fn recover_refuses_symlinked_transaction_storage_ancestors() {
    for ancestor in ["transactions", "transaction"] {
        let root = scratch();
        let target_app = app_bundle(&root, "ChatGPT.app", "original");
        let tx = Engine::begin(&root, &target_app, "install").unwrap();
        let id = tx.install_id().to_string();
        drop(tx);

        let transactions = root.join("transactions");
        let transaction = transactions.join(&id);
        let (path, backing) = if ancestor == "transactions" {
            (transactions.clone(), root.join("transactions.backing"))
        } else {
            (
                transaction.clone(),
                transactions.join(format!("{id}.backing")),
            )
        };
        fs::rename(&path, &backing).unwrap();
        symlink(&backing, &path).unwrap();

        let error = recover(&root, &id).unwrap_err();
        assert!(
            matches!(error, TxError::Refuse { .. }),
            "symlinked {ancestor} ancestor was not refused: {error:?}"
        );
        assert_eq!(
            fs::read_to_string(target_app.join("marker")).unwrap(),
            "original"
        );
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink(),
            "recovery replaced the symlinked {ancestor} ancestor"
        );
    }
}

#[test]
fn no_live_staging_name() {
    let src = include_str!("../src/engine.rs");
    assert!(!src.contains("ChatGPT.app.live"));
}
