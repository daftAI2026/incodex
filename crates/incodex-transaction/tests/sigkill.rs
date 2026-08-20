use std::env;
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_transaction::{journal_v2, recover, recover_with, Engine, Recovery};
use sha2::{Digest, Sha256};

const CHILD_MODE: &str = "INCODEX_TX_SIGKILL_CHILD";
const ROOT_ENV: &str = "INCODEX_TX_SIGKILL_ROOT";
const APP_ENV: &str = "INCODEX_TX_SIGKILL_APP";
const CANDIDATE_ENV: &str = "INCODEX_TX_SIGKILL_CANDIDATE";
const POINT_ENV: &str = "INCODEX_TX_SIGKILL_POINT";
const ID_FILE_ENV: &str = "INCODEX_TX_SIGKILL_ID_FILE";
const INSTALL_ID_ENV: &str = "INCODEX_TX_SIGKILL_INSTALL_ID";
const RECOVER_CHILD_MODE: &str = "INCODEX_TX_SIGKILL_RECOVER_CHILD";

static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

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

    let root = scratch();
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

fn run_case(point: KillPoint) {
    let root = scratch();
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

fn mutate_child(root: &Path, app: &Path, candidate: &Path, point: KillPoint) {
    let mut tx = Engine::begin(root, app, "sigkill-test").expect("begin transaction");
    let id = tx.install_id().to_string();
    let id_file = PathBuf::from(env::var(ID_FILE_ENV).expect("mutator id file"));
    fs::write(id_file, &id).expect("publish install id");

    if matches!(point, KillPoint::Discovered) {
        kill_self();
    }

    seed_original(root, app, &id);
    tx.mark_backup_committed()
        .expect("commit backup snapshot");
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

fn scratch() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let seq = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let root = env::temp_dir().join(format!("incodex-tx-sigkill-{now}-{seq}"));
    fs::create_dir_all(&root).expect("scratch");
    root
}

fn make_app(root: &Path, name: &str, marker: &str) -> PathBuf {
    let app = root.join(name);
    fs::create_dir_all(app.join("Contents/MacOS")).expect("app dirs");
    fs::create_dir_all(app.join("Contents/Resources/nested")).expect("resource dirs");
    fs::write(app.join("marker"), format!("{marker}\n")).expect("marker");
    fs::write(
        app.join("Contents/MacOS/ChatGPT"),
        format!("binary-{marker}\n"),
    )
    .expect("binary");
    fs::write(
        app.join("Contents/Resources/nested/config.json"),
        format!("{{\"marker\":\"{marker}\"}}\n"),
    )
    .expect("resource");
    symlink(
        "nested/config.json",
        app.join("Contents/Resources/current-config.json"),
    )
    .expect("resource symlink");
    app
}

fn seed_original(root: &Path, app: &Path, install_id: &str) {
    let destination = root
        .join("transactions")
        .join(install_id)
        .join("original/ChatGPT.app");
    copy_tree(app, &destination);
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("copy destination");
    for entry in fs::read_dir(from).expect("copy source") {
        let entry = entry.expect("copy entry");
        let source = entry.path();
        let destination = to.join(entry.file_name());
        let file_type = entry.file_type().expect("copy type");
        if file_type.is_symlink() {
            symlink(
                fs::read_link(&source).expect("copy link target"),
                &destination,
            )
            .expect("copy symlink");
        } else if file_type.is_dir() {
            copy_tree(&source, &destination);
        } else {
            fs::copy(source, destination).expect("copy file");
        }
    }
}

fn tree_digest(root: &Path) -> String {
    let mut entries = Vec::new();
    collect_files(root, root, &mut entries);
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut digest = Sha256::new();
    for (relative, bytes) in entries {
        digest.update(relative.as_bytes());
        digest.update([0]);
        digest.update(bytes);
        digest.update([0]);
    }
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn collect_files(root: &Path, current: &Path, entries: &mut Vec<(String, Vec<u8>)>) {
    for entry in fs::read_dir(current).expect("digest directory") {
        let entry = entry.expect("digest entry");
        let path = entry.path();
        let file_type = entry.file_type().expect("digest type");
        if file_type.is_symlink() {
            let relative = path
                .strip_prefix(root)
                .expect("digest relative link")
                .to_string_lossy()
                .into_owned();
            let target = fs::read_link(&path)
                .expect("digest link target")
                .to_string_lossy()
                .into_owned()
                .into_bytes();
            entries.push((relative, target));
        } else if file_type.is_dir() {
            collect_files(root, &path, entries);
        } else {
            let relative = path
                .strip_prefix(root)
                .expect("digest relative path")
                .to_string_lossy()
                .into_owned();
            entries.push((relative, fs::read(path).expect("digest file")));
        }
    }
}
