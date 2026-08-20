use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_transaction::{journal_v2, recover, Engine};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("incodex-reconstructed-guards-{now}-{seq}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn app(root: &Path) -> PathBuf {
    let target = root.join("ChatGPT.app");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("marker"), "live").unwrap();
    target
}

#[test]
fn recover_rejects_restore_and_trash_symlink_escapes_before_cleanup() {
    for (name, leaf) in [("restore", false), ("trash", true)] {
        let root = scratch();
        let target = app(&root);
        let victim = root.join(format!("victim-{name}"));
        fs::create_dir_all(&victim).unwrap();
        fs::write(victim.join("keep"), "must survive").unwrap();
        let tx = Engine::begin(&root, &target, "reconstructed-guard-test").unwrap();
        let id = tx.install_id().to_string();
        let hardcoded = root.join("transactions").join(&id).join(name);
        if leaf {
            fs::create_dir_all(&hardcoded).unwrap();
            symlink(&victim, hardcoded.join("ChatGPT.app")).unwrap();
        } else {
            symlink(&victim, &hardcoded).unwrap();
        }
        drop(tx);

        let error = recover(&root, &id).expect_err("hardcoded cleanup path escaped");
        assert!(error.to_string().contains("symlink"), "{error}");
        assert_eq!(
            fs::read_to_string(victim.join("keep")).unwrap(),
            "must survive"
        );
        assert_eq!(journal_v2(&root, &id).unwrap().phase, "DISCOVERED");
    }
}
