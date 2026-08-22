use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_transaction::Engine;

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

#[test]
fn begin_quiescence_refusal_happens_after_lock_before_digest_or_journal() {
    let root = scratch();
    let target = app(&root);
    let before = fs::read(target.join("marker")).unwrap();
    let error = Engine::begin_with_quiescence(&root, &target, "test", |_target| {
        Err("fixture app is still running".into())
    })
    .unwrap_err();

    assert!(error.contains("fixture app is still running"), "{error}");
    assert_eq!(fs::read(target.join("marker")).unwrap(), before);
    assert!(
        !root.join("transactions").exists()
            || fs::read_dir(root.join("transactions")).unwrap().next().is_none(),
        "quiescence refusal must not create a journal"
    );
    fs::remove_dir_all(root).unwrap();
}
