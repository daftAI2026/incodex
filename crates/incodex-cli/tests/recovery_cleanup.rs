use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_transaction::Engine;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("incodex-cleanup-{n}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn app(root: &Path, name: &str, marker: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("marker"), marker).unwrap();
    path
}

fn seed_backup(root: &Path, id: &str, source: &Path) {
    let original = root
        .join("transactions")
        .join(id)
        .join("original/ChatGPT.app");
    fs::create_dir_all(&original).unwrap();
    fs::copy(source.join("marker"), original.join("marker")).unwrap();
}

#[test]
fn recover_committed_cleanup_converges_for_directory_file_and_symlink_leftovers() {
    for kind in ["dir", "file", "symlink", "socket"] {
        let home = scratch();
        let root = home.join(".incodex");
        let target = app(&home, "ChatGPT.app", "patched");
        let candidate = app(&home, "candidate.app", "candidate");
        let mut tx = Engine::begin(&root, &target, "cleanup-test").unwrap();
        let id = tx.install_id().to_string();
        seed_backup(&root, &id, &target);
        tx.mark_backup_committed().unwrap();
        tx.place_staging(&candidate).unwrap();
        tx.swap().unwrap();
        tx.commit().unwrap();
        let outgoing = tx.outgoing_app();
        drop(tx);

        let victim = home.join(format!("victim-{kind}"));
        match kind {
            "dir" => {
                fs::create_dir_all(&outgoing).unwrap();
                fs::write(outgoing.join("leftover"), b"garbage").unwrap();
            }
            "file" => fs::write(&outgoing, b"garbage").unwrap(),
            "symlink" => {
                fs::write(&victim, b"must survive").unwrap();
                symlink(&victim, &outgoing).unwrap();
            }
            "socket" => {}
            _ => unreachable!(),
        }

        let _socket = if kind == "socket" {
            let socket_path =
                std::env::temp_dir().join(format!("incodex-cleanup-socket-{}", std::process::id()));
            let listener = UnixListener::bind(&socket_path).unwrap();
            fs::rename(&socket_path, &outgoing).unwrap();
            Some(listener)
        } else {
            None
        };

        let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
            .args(["recover", "--transaction", &id])
            .env("HOME", &home)
            .env("TERM", "dumb")
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{kind} cleanup failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!fs::symlink_metadata(&outgoing).is_ok(), "{kind} survived");
        if kind == "symlink" {
            assert_eq!(fs::read(&victim).unwrap(), b"must survive");
        }
    }
}
