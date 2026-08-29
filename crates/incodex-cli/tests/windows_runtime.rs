#![cfg(target_os = "windows")]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::windows_runtime::{publish_windows_runtime, windows_runtime_files};
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
    assert_eq!(first.files.len(), windows_runtime_files().count());
    assert!(first.pointer.ends_with("runtime/current.json"));
    let release_name = first
        .release_dir
        .file_name()
        .and_then(|name| name.to_str())
        .expect("UTF-8 Runtime release name");
    let (version, digest) = release_name
        .split_once('-')
        .expect("versioned Runtime release name");
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
    assert_eq!(digest.len(), 64, "release uses one combined SHA-256");
    assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));

    let directories = [
        user_root.clone(),
        user_root.join("runtime"),
        user_root.join("runtime/releases"),
        first.release_dir.clone(),
    ];
    for directory in &directories {
        verify_private_acl(directory).expect("private Runtime directory ACL");
    }
    for name in windows_runtime_files() {
        let path = first.release_dir.join(name);
        assert!(path.is_file(), "missing shared Runtime artifact {name}");
        verify_private_acl(&path).expect("private Runtime file ACL");
    }
    verify_private_acl(&first.pointer).expect("private Runtime pointer ACL");

    fs::remove_dir_all(user_root).expect("remove Runtime fixture");
}
