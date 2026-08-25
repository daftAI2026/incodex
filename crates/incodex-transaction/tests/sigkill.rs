use std::env;
use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use incodex_transaction::{
    journal_v2, recover, recover_with, restore_committed_with_checkpoint, Engine, Recovery,
};

#[path = "support/sigkill.rs"]
mod support;
use support::{make_app, scratch, seed_original, tree_digest};

const CHILD_MODE: &str = "INCODEX_TX_SIGKILL_CHILD";
const ROOT_ENV: &str = "INCODEX_TX_SIGKILL_ROOT";
const APP_ENV: &str = "INCODEX_TX_SIGKILL_APP";
const CANDIDATE_ENV: &str = "INCODEX_TX_SIGKILL_CANDIDATE";
const POINT_ENV: &str = "INCODEX_TX_SIGKILL_POINT";
const ID_FILE_ENV: &str = "INCODEX_TX_SIGKILL_ID_FILE";
const INSTALL_ID_ENV: &str = "INCODEX_TX_SIGKILL_INSTALL_ID";
const RECOVER_CHILD_MODE: &str = "INCODEX_TX_SIGKILL_RECOVER_CHILD";
const UNINSTALL_CHILD_MODE: &str = "INCODEX_TX_SIGKILL_UNINSTALL_CHILD";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KillPoint {
    Discovered,
    Staged,
    TargetMovedOut,
    Swapped,
    Committed,
}

impl KillPoint {
    const ALL: [Self; 5] = [
        Self::Discovered,
        Self::Staged,
        Self::TargetMovedOut,
        Self::Swapped,
        Self::Committed,
    ];

    fn as_env(self) -> &'static str {
        match self {
            Self::Discovered => "DISCOVERED",
            Self::Staged => "STAGED",
            Self::TargetMovedOut => "TARGET_MOVED_OUT",
            Self::Swapped => "SWAPPED",
            Self::Committed => "COMMITTED",
        }
    }

    fn journal_phase(self) -> &'static str {
        self.as_env()
    }

    fn has_backup(self) -> bool {
        !matches!(self, Self::Discovered)
    }

    fn should_commit(self) -> bool {
        matches!(self, Self::Committed)
    }
}

#[test]
fn sigkill_recovery_uses_real_transaction_checkpoints() {
    if run_child_mode() {
        return;
    }

    for point in KillPoint::ALL {
        run_case(point);
    }
}

#[test]
fn recover_sigkill_restarts_from_a_real_recovery_step() {
    if run_recover_child_mode() {
        return;
    }

    let root = scratch("sigkill");
    let app = make_app(&root, "ChatGPT.app", "ORIGINAL");
    let candidate = make_app(&root, "candidate.app", "PATCHED");
    let original_digest = tree_digest(&app);
    let id_file = root.join("install-id");

    let status = spawn_child(
        "mutate",
        KillPoint::Swapped,
        &root,
        &app,
        &candidate,
        Some(&id_file),
        None,
    );
    assert_eq!(
        status.signal(),
        Some(libc::SIGKILL),
        "mutator was not SIGKILLed: {status:?}"
    );

    let install_id = fs::read_to_string(&id_file).expect("child must publish install id");
    let install_id = install_id.trim();

    // +--------------------------------------------------------------------+
    // | 这里只在真实恢复步骤完成后杀死进程；不伪造 copy/rename/remove 的内核中点。 |
    // | 系统调用内部断电模型保留为后续待办。                                 |
    // +--------------------------------------------------------------------+
    let status = spawn_recover_child("kill-after-restore", &root, install_id);
    assert_eq!(
        status.signal(),
        Some(libc::SIGKILL),
        "recover child was not SIGKILLed: {status:?}"
    );
    assert_eq!(
        journal_v2(&root, install_id)
            .expect("journal survives a killed recover")
            .phase,
        "SWAPPED"
    );

    let status = spawn_recover_child("recover", &root, install_id);
    assert_eq!(status.code(), Some(0), "second recover failed: {status:?}");

    let recovered = journal_v2(&root, install_id).expect("recovery keeps the journal readable");
    assert_eq!(recovered.phase, "ROLLED_BACK");
    assert_eq!(tree_digest(&app), original_digest);

    let tx_dir = root.join("transactions").join(install_id);
    assert!(!tx_dir.join("staging/ChatGPT.app").exists());
    assert!(!tx_dir.join("outgoing/ChatGPT.app").exists());
    assert!(!tx_dir.join("trash/ChatGPT.app").exists());
    let original = tx_dir.join("original/ChatGPT.app");
    assert!(original.exists());
    assert_eq!(tree_digest(&original), original_digest);
}

#[test]
fn uninstall_sigkill_after_live_moves_to_trash_recovers() {
    if run_uninstall_child_mode() {
        return;
    }

    let root = scratch("sigkill");
    let app = make_app(&root, "ChatGPT.app", "ORIGINAL");
    let candidate = make_app(&root, "candidate.app", "PATCHED");
    let original_digest = tree_digest(&app);
    let id_file = root.join("install-id");
    let status = spawn_uninstall_child(
        "uninstall",
        &root,
        &app,
        Some(&candidate),
        Some(&id_file),
        None,
    );
    assert_eq!(
        status.signal(),
        Some(libc::SIGKILL),
        "uninstall child was not SIGKILLed: {status:?}"
    );

    let install_id = fs::read_to_string(&id_file).unwrap().trim().to_string();
    assert_eq!(
        journal_v2(&root, &install_id).unwrap().phase,
        "UNINSTALLING"
    );
    assert!(!app.exists(), "live survived the intentional rename gap");

    let status = spawn_uninstall_child("recover", &root, &app, None, None, Some(&install_id));
    assert_eq!(status.code(), Some(0), "recover child failed: {status:?}");
    assert_eq!(journal_v2(&root, &install_id).unwrap().phase, "ROLLED_BACK");
    assert_eq!(tree_digest(&app), original_digest);

    let tx_dir = root.join("transactions").join(&install_id);
    for relative in [
        "staging/ChatGPT.app",
        "outgoing/ChatGPT.app",
        "restore/ChatGPT.app",
        "trash/ChatGPT.app",
    ] {
        assert!(!tx_dir.join(relative).exists(), "leftover: {relative}");
    }
    assert_eq!(
        tree_digest(&tx_dir.join("original/ChatGPT.app")),
        original_digest
    );
}

fn run_case(point: KillPoint) {
    let root = scratch("sigkill");
    let app = make_app(&root, "ChatGPT.app", "ORIGINAL");
    let candidate = make_app(&root, "candidate.app", "PATCHED");
    let original_digest = tree_digest(&app);
    let patched_digest = tree_digest(&candidate);
    let id_file = root.join("install-id");

    let status = spawn_child(
        "mutate",
        point,
        &root,
        &app,
        &candidate,
        Some(&id_file),
        None,
    );
    assert_eq!(
        status.signal(),
        Some(libc::SIGKILL),
        "{point:?} child was not SIGKILLed: {status:?}"
    );

    let install_id = fs::read_to_string(&id_file).expect("child must publish install id");
    let install_id = install_id.trim();
    let journal = journal_v2(&root, install_id).expect("SIGKILL leaves a valid journal");
    assert_eq!(journal.phase, point.journal_phase());
    if !point.should_commit() {
        assert_ne!(
            journal.phase, "COMMITTED",
            "unfinished child lied in its journal"
        );
    }

    let status = spawn_child(
        "recover",
        point,
        &root,
        &app,
        &candidate,
        None,
        Some(install_id),
    );
    assert_eq!(status.code(), Some(0), "recover child failed: {status:?}");

    let recovered = journal_v2(&root, install_id).expect("recovery keeps the journal readable");
    if point.should_commit() {
        assert_eq!(recovered.phase, "COMMITTED");
        assert_eq!(
            tree_digest(&app),
            patched_digest,
            "commit left a partial app"
        );
    } else {
        assert_eq!(recovered.phase, "ROLLED_BACK");
        assert_eq!(
            tree_digest(&app),
            original_digest,
            "recovery left a partial app"
        );
        assert_ne!(tree_digest(&app), patched_digest);
    }
    let current_config = app.join("Contents/Resources/current-config.json");
    assert!(
        fs::symlink_metadata(&current_config)
            .expect("restored config link")
            .file_type()
            .is_symlink(),
        "{point:?}: recovery replaced an app-bundle symlink with a regular file"
    );
    assert_eq!(
        fs::read_link(&current_config).expect("config link target"),
        PathBuf::from("nested/config.json")
    );

    let tx_dir = root.join("transactions").join(install_id);
    assert!(!tx_dir.join("staging/ChatGPT.app").exists());
    assert!(!tx_dir.join("outgoing/ChatGPT.app").exists());
    let original = tx_dir.join("original/ChatGPT.app");
    if point.has_backup() {
        assert!(
            original.exists(),
            "{point:?}: recovery consumed the durable backup"
        );
        assert_eq!(tree_digest(&original), original_digest);
    } else {
        assert!(!original.exists());
    }
}

fn run_child_mode() -> bool {
    let Some(mode) = env::var(CHILD_MODE).ok() else {
        return false;
    };
    let root = PathBuf::from(env::var(ROOT_ENV).expect("child root"));
    let app = PathBuf::from(env::var(APP_ENV).expect("child app"));
    let candidate = PathBuf::from(env::var(CANDIDATE_ENV).expect("child candidate"));
    let point = parse_point(&env::var(POINT_ENV).expect("child point"));
    match mode.as_str() {
        "mutate" => mutate_child(&root, &app, &candidate, point),
        "recover" => recover_child(
            &root,
            &env::var(INSTALL_ID_ENV).expect("recover install id"),
            point,
        ),
        other => panic!("unknown child mode {other}"),
    }
    true
}

fn run_recover_child_mode() -> bool {
    let Some(mode) = env::var(RECOVER_CHILD_MODE).ok() else {
        return false;
    };
    let root = PathBuf::from(env::var(ROOT_ENV).expect("recover child root"));
    let install_id = env::var(INSTALL_ID_ENV).expect("recover child install id");
    match mode.as_str() {
        "kill-after-restore" => {
            recover_with(&root, &install_id, |_| {
                kill_self();
            })
            .expect("recover child");
        }
        "recover" => {
            recover(&root, &install_id).expect("recover child");
        }
        other => panic!("unknown recover child mode {other}"),
    }
    true
}

fn run_uninstall_child_mode() -> bool {
    let Some(mode) = env::var(UNINSTALL_CHILD_MODE).ok() else {
        return false;
    };
    let root = PathBuf::from(env::var(ROOT_ENV).expect("uninstall child root"));
    let app = PathBuf::from(env::var(APP_ENV).expect("uninstall child app"));
    match mode.as_str() {
        "uninstall" => {
            let candidate = PathBuf::from(env::var(CANDIDATE_ENV).expect("uninstall candidate"));
            let mut tx = Engine::begin(&root, &app, "sigkill-uninstall-test")
                .expect("begin uninstall transaction");
            let id = tx.install_id().to_string();
            fs::write(
                PathBuf::from(env::var(ID_FILE_ENV).expect("uninstall id file")),
                &id,
            )
            .expect("publish uninstall id");
            seed_original(&root, &app, &id);
            tx.mark_backup_committed().expect("commit backup snapshot");
            tx.place_staging(&candidate).expect("stage candidate");
            tx.swap().expect("swap candidate");
            tx.commit().expect("commit candidate");
            drop(tx);
            restore_committed_with_checkpoint(&root, &id, &app, |phase| {
                if phase == "LIVE_MOVED_TO_TRASH_DURABLE" {
                    kill_self();
                }
            })
            .expect("restore committed app");
        }
        "recover" => {
            let id = env::var(INSTALL_ID_ENV).expect("recover uninstall id");
            recover(&root, &id).expect("recover uninstall transaction");
        }
        other => panic!("unknown uninstall child mode {other}"),
    }
    true
}

fn mutate_child(root: &Path, app: &Path, candidate: &Path, point: KillPoint) {
    let mut tx = Engine::begin(root, app, "sigkill-test").expect("begin transaction");
    let id = tx.install_id().to_string();
    let id_file = PathBuf::from(env::var(ID_FILE_ENV).expect("mutator id file"));
    fs::write(id_file, &id).expect("publish install id");

    if matches!(point, KillPoint::Discovered) {
        kill_self();
    }

    seed_original(root, app, &id);
    tx.mark_backup_committed().expect("commit backup snapshot");
    let staged = candidate.to_path_buf();
    tx.place_staging(&staged).expect("stage candidate");
    if matches!(point, KillPoint::Staged) {
        kill_self();
    }

    if matches!(point, KillPoint::TargetMovedOut | KillPoint::Swapped) {
        tx.swap_with_checkpoint(|phase| {
            if (point == KillPoint::TargetMovedOut && phase == "TARGET_MOVED_OUT")
                || (point == KillPoint::Swapped && phase == "SWAPPED")
            {
                kill_self();
            }
        })
        .expect("swap candidate");
    } else {
        tx.swap().expect("swap candidate");
    }

    if matches!(point, KillPoint::Committed) {
        tx.commit().expect("commit candidate");
        kill_self();
    }

    panic!("mutator did not reach requested kill point {point:?}");
}

fn recover_child(root: &Path, id: &str, point: KillPoint) {
    let result = recover(root, id).expect("recover transaction");
    let expected = if point.should_commit() {
        Recovery::Done
    } else {
        Recovery::Rollback
    };
    assert_eq!(result.action, expected);
}

fn spawn_child(
    mode: &str,
    point: KillPoint,
    root: &Path,
    app: &Path,
    candidate: &Path,
    id_file: Option<&Path>,
    install_id: Option<&str>,
) -> ExitStatus {
    let exe = env::current_exe().expect("test executable");
    let mut command = Command::new(exe);
    command
        .args([
            "--exact",
            "sigkill_recovery_uses_real_transaction_checkpoints",
            "--nocapture",
        ])
        .env(CHILD_MODE, mode)
        .env(ROOT_ENV, root)
        .env(APP_ENV, app)
        .env(CANDIDATE_ENV, candidate)
        .env(POINT_ENV, point.as_env())
        .env(ID_FILE_ENV, id_file.unwrap_or(Path::new("")))
        .env(INSTALL_ID_ENV, install_id.unwrap_or(""))
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    command.status().expect("spawn transaction child")
}

fn spawn_recover_child(mode: &str, root: &Path, install_id: &str) -> ExitStatus {
    let exe = env::current_exe().expect("test executable");
    Command::new(exe)
        .args([
            "--exact",
            "recover_sigkill_restarts_from_a_real_recovery_step",
            "--nocapture",
        ])
        .env(RECOVER_CHILD_MODE, mode)
        .env(ROOT_ENV, root)
        .env(INSTALL_ID_ENV, install_id)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .expect("spawn recover child")
}

fn spawn_uninstall_child(
    mode: &str,
    root: &Path,
    app: &Path,
    candidate: Option<&Path>,
    id_file: Option<&Path>,
    install_id: Option<&str>,
) -> ExitStatus {
    let exe = env::current_exe().expect("uninstall child executable");
    Command::new(exe)
        .args([
            "--exact",
            "uninstall_sigkill_after_live_moves_to_trash_recovers",
            "--nocapture",
        ])
        .env(UNINSTALL_CHILD_MODE, mode)
        .env(ROOT_ENV, root)
        .env(APP_ENV, app)
        .env(CANDIDATE_ENV, candidate.unwrap_or(Path::new("")))
        .env(ID_FILE_ENV, id_file.unwrap_or(Path::new("")))
        .env(INSTALL_ID_ENV, install_id.unwrap_or(""))
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .expect("spawn uninstall child")
}

fn parse_point(value: &str) -> KillPoint {
    KillPoint::ALL
        .into_iter()
        .find(|point| point.as_env() == value)
        .unwrap_or_else(|| panic!("unknown kill point {value}"))
}

fn kill_self() -> ! {
    let result = unsafe { libc::kill(libc::getpid(), libc::SIGKILL) };
    assert_eq!(result, 0, "SIGKILL child");
    // +---------------------------------------------------------------+
    // | kill(2) 可能先返回、信号随后才送达；保持进程存活，避免误成 SIGABRT。 |
    // +---------------------------------------------------------------+
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
