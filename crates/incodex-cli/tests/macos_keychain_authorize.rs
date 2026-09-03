#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "incodex-macos-keychain-authorize-{}-{sequence}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn explicit_authorize_mode_reads_once_but_never_emits_the_storage_key() {
    let home = scratch();
    let helper = compile_helper_with_fake_security(&home);
    let marker = home.join("query-called");

    let output = Command::new(&helper)
        .arg("--authorize")
        .env("INCODEX_FAKE_SECURITY_MARKER", &marker)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "status={:?} stdout={} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "authorize mode leaked key bytes");
    assert!(output.stderr.is_empty());
    assert_eq!(fs::read(&marker).unwrap(), b"queried");

    let output = Command::new(&helper)
        .args(["--authorize", "unexpected"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());

    fs::remove_dir_all(home).unwrap();
}

fn compile_helper_with_fake_security(home: &Path) -> PathBuf {
    let clang = command_stdout("xcrun", &["--find", "clang"]);
    let sdk = command_stdout("xcrun", &["--sdk", "macosx", "--show-sdk-path"]);
    let fake_source = home.join("fake_security.c");
    fs::write(
        &fake_source,
        r#"
#include <CoreFoundation/CoreFoundation.h>
#include <Security/Security.h>
#include <fcntl.h>
#include <stdlib.h>
#include <unistd.h>

OSStatus SecItemCopyMatching(CFDictionaryRef query, CFTypeRef *result) {
    (void)query;
    const char *marker = getenv("INCODEX_FAKE_SECURITY_MARKER");
    if (marker) {
        int fd = open(marker, O_WRONLY | O_CREAT | O_TRUNC, 0600);
        if (fd >= 0) {
            (void)write(fd, "queried", 7);
            close(fd);
        }
    }
    const unsigned char bytes[] = "synthetic-secret-must-not-leak";
    *result = CFDataCreate(kCFAllocatorDefault, bytes, sizeof(bytes) - 1);
    return *result ? errSecSuccess : errSecAllocate;
}
"#,
    )
    .unwrap();
    let fake = home.join("libAuthorizeFakeSecurity.dylib");
    let status = Command::new(&clang)
        .args(["-isysroot", &sdk, "-Wall", "-Wextra", "-Werror"])
        .args(["-dynamiclib", "-framework", "CoreFoundation"])
        .arg(&fake_source)
        .args([
            "-Wl,-install_name,@rpath/libAuthorizeFakeSecurity.dylib",
            "-o",
        ])
        .arg(&fake)
        .status()
        .unwrap();
    assert!(status.success(), "fake Security library did not compile");

    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("native/macos_keychain_helper.c");
    let helper = home.join("incodex-keychain-helper-test");
    let status = Command::new(&clang)
        .args(["-isysroot", &sdk, "-Wall", "-Wextra", "-Werror"])
        .arg(&source)
        .arg(&fake)
        .args(["-framework", "CoreFoundation", "-framework", "Security"])
        .args(["-Wl,-rpath,@executable_path", "-o"])
        .arg(&helper)
        .status()
        .unwrap();
    assert!(status.success(), "test Keychain helper did not compile");
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o700)).unwrap();
    helper
}

fn command_stdout(command: &str, arguments: &[&str]) -> String {
    let output = Command::new(command).args(arguments).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
