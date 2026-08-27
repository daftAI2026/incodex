#![cfg(target_os = "windows")]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::windows_helper::publish_windows_helper;
use incodex_core::windows_session::verify_private_acl;

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

const WINDOWS_GUI_SUBSYSTEM: u16 = 2;

fn pe_subsystem(bytes: &[u8]) -> u16 {
    assert_eq!(&bytes[0..2], b"MZ", "fixture must be a PE executable");
    let pe_offset = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
    assert_eq!(&bytes[pe_offset..pe_offset + 4], b"PE\0\0");
    let optional_header = pe_offset + 24;
    u16::from_le_bytes(
        bytes[optional_header + 68..optional_header + 70]
            .try_into()
            .unwrap(),
    )
}

fn scratch_root() -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "incodex-windows-helper-{}-{sequence}",
        std::process::id()
    ))
}

#[test]
fn publishes_a_windowless_private_content_addressed_helper_without_mutating_the_cli() {
    let user_root = scratch_root();
    let source = std::env::current_exe().expect("test helper source");
    let source_before = fs::read(&source).unwrap();
    let first = publish_windows_helper(&user_root, &source).expect("publish Windows helper");
    let second = publish_windows_helper(&user_root, &source).expect("reuse Windows helper");

    assert_eq!(first, second);
    assert_ne!(first.executable, source);
    assert_eq!(fs::read(&source).unwrap(), source_before);
    assert_eq!(pe_subsystem(&fs::read(&first.executable).unwrap()), WINDOWS_GUI_SUBSYSTEM);
    assert_ne!(fs::read(&first.executable).unwrap(), source_before);
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
