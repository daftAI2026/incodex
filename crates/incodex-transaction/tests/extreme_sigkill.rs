use std::env;
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_transaction::{journal_v2, recover, Engine};
use sha2::{Digest, Sha256};

const ROOT_ENV: &str = "INCODEX_TX_SIGKILL_ROOT";
const APP_ENV: &str = "INCODEX_TX_SIGKILL_APP";
const CANDIDATE_ENV: &str = "INCODEX_TX_SIGKILL_CANDIDATE";
const POINT_ENV: &str = "INCODEX_TX_SIGKILL_POINT";
const ID_FILE_ENV: &str = "INCODEX_TX_SIGKILL_ID_FILE";
const INSTALL_ID_ENV: &str = "INCODEX_TX_SIGKILL_INSTALL_ID";
const CHILD_MODE_ENV: &str = "INCODEX_TX_EXTREME_CHILD";

static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwapGap {
    LiveMovedOut,
    StagingMovedIn,
}

impl SwapGap {
    const ALL: [Self; 2] = [Self::LiveMovedOut, Self::StagingMovedIn];

    fn as_str(self) -> &'static str {
        match self {
            Self::LiveMovedOut => "LIVE_MOVED_OUT",
            Self::StagingMovedIn => "STAGING_MOVED_IN",
        }
    }
}

#[test]
fn child_entry() {
    let _ = run_child_mode();
}

#[test]
fn pre_swap_recover_does_not_replace_live_with_partial_original() {
    let root = scratch();
    let app = make_app(&root, "ChatGPT.app", "ORIGINAL");
    let original_digest = tree_digest(&app);
    let id_file = root.join("install-id");

    let status = spawn_child("discover-partial", &root, &app, None, &id_file, None, None);
    assert_eq!(
        status.signal(),
        Some(libc::SIGKILL),
        "install child was not SIGKILLed: {status:?}"
    );

    let install_id = read_install_id(&id_file);
    assert_eq!(journal_v2(&root, &install_id).unwrap().phase, "DISCOVERED");

    let status = spawn_child(
        "recover",
        &root,
        &app,
        None,
        Path::new(""),
        None,
        Some(&install_id),
    );
    assert_eq!(status.code(), Some(0), "recover child failed: {status:?}");
    assert_eq!(tree_digest(&app), original_digest);
    assert_eq!(journal_v2(&root, &install_id).unwrap().phase, "ROLLED_BACK");
    assert!(!root
        .join("transactions")
        .join(&install_id)
        .join("original/ChatGPT.app")
        .exists());
}

#[test]
fn recover_never_uses_a_partial_outgoing_as_the_rollback_source() {
    let root = scratch();
    let app = make_app(&root, "ChatGPT.app", "ORIGINAL");
    let candidate = make_app(&root, "candidate.app", "PATCHED");
    let original_digest = tree_digest(&app);
    let id_file = root.join("install-id");

    let status = spawn_child(
        "partial-outgoing",
        &root,
        &app,
        Some(&candidate),
        &id_file,
        None,
        None,
    );
    assert_eq!(
        status.signal(),
        Some(libc::SIGKILL),
        "commit cleanup child was not SIGKILLed: {status:?}"
    );

    let install_id = read_install_id(&id_file);
    assert_eq!(journal_v2(&root, &install_id).unwrap().phase, "SWAPPED");

    let status = spawn_child(
        "recover",
        &root,
        &app,
        None,
        Path::new(""),
        None,
        Some(&install_id),
    );
    assert_eq!(status.code(), Some(0), "recover child failed: {status:?}");
    assert_eq!(
        tree_digest(&app),
        original_digest,
        "recovery restored a partial outgoing bundle"
    );
}

#[test]
fn committed_journal_survives_kill_before_outgoing_cleanup() {
    let root = scratch();
    let app = make_app(&root, "ChatGPT.app", "ORIGINAL");
    let candidate = make_app(&root, "candidate.app", "PATCHED");
    let original_digest = tree_digest(&app);
    let patched_digest = tree_digest(&candidate);
    let id_file = root.join("install-id");

    let status = spawn_child(
        "commit-kill",
        &root,
        &app,
        Some(&candidate),
        &id_file,
        None,
        None,
    );
    assert_eq!(
        status.signal(),
        Some(libc::SIGKILL),
        "commit child was not SIGKILLed: {status:?}"
    );

    let install_id = read_install_id(&id_file);
    assert_eq!(journal_v2(&root, &install_id).unwrap().phase, "COMMITTED");

    let status = spawn_child(
        "recover",
        &root,
        &app,
        None,
        Path::new(""),
        None,
        Some(&install_id),
    );
    assert_eq!(status.code(), Some(0), "recover child failed: {status:?}");
    assert_eq!(journal_v2(&root, &install_id).unwrap().phase, "COMMITTED");
    assert_eq!(tree_digest(&app), patched_digest);

    let tx_dir = root.join("transactions").join(&install_id);
    assert!(!tx_dir.join("outgoing/ChatGPT.app").exists());
    assert_eq!(
        tree_digest(&tx_dir.join("original/ChatGPT.app")),
        original_digest
    );
}

#[test]
fn swap_sigkill_between_real_renames_recovers_a_complete_original() {
    for gap in SwapGap::ALL {
        let root = scratch();
        let app = make_app(&root, "ChatGPT.app", "ORIGINAL");
        let candidate = make_app(&root, "candidate.app", "PATCHED");
        let original_digest = tree_digest(&app);
        let id_file = root.join("install-id");

        let status = spawn_child(
            "swap-gap",
            &root,
            &app,
            Some(&candidate),
            &id_file,
            Some(gap.as_str()),
            None,
        );
        assert_eq!(
            status.signal(),
            Some(libc::SIGKILL),
            "swap gap {gap:?} child was not SIGKILLed: {status:?}"
        );

        let install_id = read_install_id(&id_file);
        assert_eq!(
            journal_v2(&root, &install_id).unwrap().phase,
            "TARGET_MOVED_OUT"
        );

        let status = spawn_child(
            "recover",
            &root,
            &app,
            None,
            Path::new(""),
            None,
            Some(&install_id),
        );
        assert_eq!(status.code(), Some(0), "recover child failed: {status:?}");
        assert_eq!(tree_digest(&app), original_digest);

        let tx_dir = root.join("transactions").join(&install_id);
        assert!(!tx_dir.join("staging/ChatGPT.app").exists());
        assert!(!tx_dir.join("outgoing/ChatGPT.app").exists());
        assert_eq!(
            tree_digest(&tx_dir.join("original/ChatGPT.app")),
            original_digest
        );
    }
}

fn run_child_mode() -> bool {
    let Some(mode) = env::var(CHILD_MODE_ENV).ok() else {
        return false;
    };
    let root = PathBuf::from(env::var(ROOT_ENV).expect("child root"));
    match mode.as_str() {
        "discover-partial" => {
            let app = PathBuf::from(env::var(APP_ENV).expect("partial install app"));
            let tx = Engine::begin(&root, &app, "sigkill-test").expect("begin transaction");
            let id = tx.install_id().to_string();
            let id_file = PathBuf::from(env::var(ID_FILE_ENV).expect("partial install id file"));
            fs::write(id_file, &id).expect("publish install id");
            let partial = root
                .join("transactions")
                .join(&id)
                .join("original/ChatGPT.app");
            fs::create_dir_all(&partial).expect("create partial backup");
            fs::write(partial.join("marker"), "PARTIAL\n").expect("write partial backup");
            kill_self();
        }
        "partial-outgoing" => {
            let app = PathBuf::from(env::var(APP_ENV).expect("partial transaction app"));
            let candidate =
                PathBuf::from(env::var(CANDIDATE_ENV).expect("partial transaction candidate"));
            let mut tx = Engine::begin(&root, &app, "sigkill-test").expect("begin transaction");
            let id = tx.install_id().to_string();
            let id_file =
                PathBuf::from(env::var(ID_FILE_ENV).expect("partial transaction id file"));
            fs::write(id_file, &id).expect("publish install id");
            seed_original(&root, &app, &id);
            tx.place_staging(&candidate).expect("stage candidate");
            tx.swap().expect("swap candidate");
            fs::remove_file(tx.outgoing_app().join("Contents/MacOS/ChatGPT"))
                .expect("remove one outgoing file");
            kill_self();
        }
        "commit-kill" => {
            let app = PathBuf::from(env::var(APP_ENV).expect("commit app"));
            let candidate = PathBuf::from(env::var(CANDIDATE_ENV).expect("commit candidate"));
            let mut tx = Engine::begin(&root, &app, "sigkill-test").expect("begin transaction");
            let id = tx.install_id().to_string();
            let id_file = PathBuf::from(env::var(ID_FILE_ENV).expect("commit id file"));
            fs::write(id_file, &id).expect("publish install id");
            seed_original(&root, &app, &id);
            tx.place_staging(&candidate).expect("stage candidate");
            tx.swap().expect("swap candidate");
            tx.commit_with_checkpoint(|phase| {
                if phase == "COMMITTED_BEFORE_CLEANUP" {
                    kill_self();
                }
            })
            .expect("commit candidate");
        }
        "swap-gap" => {
            let app = PathBuf::from(env::var(APP_ENV).expect("swap app"));
            let candidate = PathBuf::from(env::var(CANDIDATE_ENV).expect("swap candidate"));
            let gap = parse_swap_gap(&env::var(POINT_ENV).expect("swap gap"));
            let mut tx = Engine::begin(&root, &app, "sigkill-test").expect("begin transaction");
            let id = tx.install_id().to_string();
            let id_file = PathBuf::from(env::var(ID_FILE_ENV).expect("swap id file"));
            fs::write(id_file, &id).expect("publish install id");
            seed_original(&root, &app, &id);
            tx.place_staging(&candidate).expect("stage candidate");
            tx.swap_with_checkpoint(|phase| {
                if phase == gap.as_str() {
                    kill_self();
                }
            })
            .expect("swap candidate");
            panic!("swap gap {gap:?} was not reached");
        }
        "recover" => {
            let install_id = env::var(INSTALL_ID_ENV).expect("recover child install id");
            recover(&root, &install_id).expect("recover child");
        }
        other => panic!("unknown extreme child mode {other}"),
    }
    true
}

fn spawn_child(
    mode: &str,
    root: &Path,
    app: &Path,
    candidate: Option<&Path>,
    id_file: &Path,
    point: Option<&str>,
    install_id: Option<&str>,
) -> ExitStatus {
    let exe = env::current_exe().expect("test executable");
    let mut command = Command::new(exe);
    command
        .args(["--exact", "child_entry", "--nocapture"])
        .env(CHILD_MODE_ENV, mode)
        .env(ROOT_ENV, root)
        .env(APP_ENV, app)
        .env(CANDIDATE_ENV, candidate.unwrap_or(Path::new("")))
        .env(POINT_ENV, point.unwrap_or(""))
        .env(ID_FILE_ENV, id_file)
        .env(INSTALL_ID_ENV, install_id.unwrap_or(""))
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    command.status().expect("spawn extreme transaction child")
}

fn parse_swap_gap(value: &str) -> SwapGap {
    SwapGap::ALL
        .into_iter()
        .find(|gap| gap.as_str() == value)
        .unwrap_or_else(|| panic!("unknown swap gap {value}"))
}

fn read_install_id(id_file: &Path) -> String {
    fs::read_to_string(id_file)
        .expect("child must publish install id")
        .trim()
        .to_string()
}

fn kill_self() -> ! {
    let result = unsafe { libc::kill(libc::getpid(), libc::SIGKILL) };
    assert_eq!(result, 0, "SIGKILL child");
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
    let root = env::temp_dir().join(format!("incodex-tx-extreme-{now}-{seq}"));
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
