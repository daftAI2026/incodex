#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::macos_keychain_assets::{bundled_helper_bytes, ensure_bundled_registration};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let sequence = SEQ.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "incodex-macos-keychain-native-{}-{sequence}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn bundled_keychain_helper_is_published_once_and_rejects_direct_invocation() {
    let home = scratch();
    let root = home.join(".incodex");
    let app = home.join("Applications/ChatGPT.app");
    fs::create_dir_all(&app).unwrap();

    let registration = ensure_bundled_registration(&root, &app).unwrap();
    let bytes = fs::read(&registration.helper_path).unwrap();
    assert_eq!(bytes, bundled_helper_bytes());
    assert_eq!(&bytes[..4], &[0xcf, 0xfa, 0xed, 0xfe]);
    assert_eq!(
        fs::metadata(&registration.helper_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );

    let output = Command::new(&registration.helper_path).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());

    let repeated = ensure_bundled_registration(&root, &app).unwrap();
    assert_eq!(repeated, registration);
    assert_eq!(fs::read(&repeated.helper_path).unwrap(), bytes);

    fs::remove_dir_all(home).unwrap();
}
