#![cfg(target_os = "windows")]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::windows_helper::publish_windows_helper;
use incodex_core::windows_session::verify_private_acl;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn scratch_root() -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "incodex-windows-helper-{}-{sequence}",
        std::process::id()
    ))
}

#[test]
fn publishes_the_running_cli_to_a_private_content_addressed_helper_path() {
    let user_root = scratch_root();
    let source = std::env::current_exe().expect("test helper source");
    let first = publish_windows_helper(&user_root, &source).expect("publish Windows helper");
    let second = publish_windows_helper(&user_root, &source).expect("reuse Windows helper");

    assert_eq!(first, second);
    assert_ne!(first.executable, source);
    assert_eq!(fs::read(&first.executable).unwrap(), fs::read(source).unwrap());
    assert_eq!(first.sha256.len(), 64);
    assert!(first.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(
        first.executable.parent().unwrap().file_name().unwrap(),
        first.sha256.as_str()
    );
    verify_private_acl(&first.executable).expect("private helper executable ACL");
    verify_private_acl(first.executable.parent().unwrap()).expect("private helper release ACL");

    fs::remove_dir_all(user_root).expect("remove Windows helper fixture");
}
