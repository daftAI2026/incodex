#![cfg(target_os = "windows")]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::windows_runtime::{publish_windows_runtime, WINDOWS_RUNTIME_FILES};
use incodex_core::windows_session::verify_private_acl;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn scratch_root() -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "incodex-windows-runtime-{}-{sequence}",
        std::process::id()
    ))
}

#[test]
fn publishes_the_shared_runtime_as_one_private_content_addressed_release() {
    let user_root = scratch_root();
    let first = publish_windows_runtime(&user_root).expect("publish shared Runtime");
    let second = publish_windows_runtime(&user_root).expect("reuse shared Runtime");

    assert_eq!(first, second, "identical Runtime must reuse one release");
    assert_eq!(first.main, first.release_dir.join("incodex-main.cjs"));
    assert_eq!(first.files.len(), WINDOWS_RUNTIME_FILES.len());
    assert!(first.pointer.ends_with("runtime/current.json"));

    let directories = [
        user_root.clone(),
        user_root.join("runtime"),
        user_root.join("runtime/releases"),
        first.release_dir.clone(),
    ];
    for directory in &directories {
        verify_private_acl(directory).expect("private Runtime directory ACL");
    }
    for name in WINDOWS_RUNTIME_FILES {
        let path = first.release_dir.join(name);
        assert!(path.is_file(), "missing shared Runtime artifact {name}");
        verify_private_acl(&path).expect("private Runtime file ACL");
    }
    verify_private_acl(&first.pointer).expect("private Runtime pointer ACL");

    fs::remove_dir_all(user_root).expect("remove Runtime fixture");
}
