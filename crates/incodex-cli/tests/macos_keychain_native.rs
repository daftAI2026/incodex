#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::macos_keychain_assets::{
    bundled_helper_bytes, bundled_provider_bytes, ensure_bundled_registration,
    install_keychain_provider,
};

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

#[test]
fn bundled_keychain_provider_is_a_normal_loader_relative_dylib() {
    let home = scratch();
    let provider = home.join("IncodexKeyProvider.dylib");
    fs::write(&provider, bundled_provider_bytes()).unwrap();

    let bytes = fs::read(&provider).unwrap();
    assert_eq!(&bytes[..4], &[0xcf, 0xfa, 0xed, 0xfe]);

    let id = Command::new("/usr/bin/otool")
        .arg("-D")
        .arg(&provider)
        .output()
        .unwrap();
    assert!(
        id.status.success(),
        "{}",
        String::from_utf8_lossy(&id.stderr)
    );
    assert!(
        String::from_utf8_lossy(&id.stdout)
            .lines()
            .any(|line| line.trim() == "@loader_path/IncodexKeyProvider.dylib"),
        "{}",
        String::from_utf8_lossy(&id.stdout)
    );

    let dependencies = Command::new("/usr/bin/otool")
        .arg("-L")
        .arg(&provider)
        .output()
        .unwrap();
    assert!(dependencies.status.success());
    let dependencies = String::from_utf8_lossy(&dependencies.stdout);
    assert!(
        dependencies.contains("Security.framework"),
        "{dependencies}"
    );
    assert!(
        dependencies.contains("CoreFoundation.framework"),
        "{dependencies}"
    );
    assert!(
        !dependencies.contains("reexport"),
        "the provider must not replace a foundational library: {dependencies}"
    );

    fs::remove_dir_all(home).unwrap();
}

#[test]
fn install_transaction_owns_provider_placement_and_failure_rollback() {
    let install = include_str!("../src/install.rs");
    let assets = include_str!("../src/macos_keychain_assets.rs");
    let macho = include_str!("../../incodex-macos/src/macho.rs");
    let implementation = format!("{install}\n{assets}\n{macho}");
    let transaction = install
        .split("let mut tx = begin_verified_transaction_with_quiescence")
        .nth(1)
        .expect("install must have one transaction body")
        .split("let commit = match tx.commit()")
        .next()
        .expect("provider mutation must happen before transaction commit");
    let provider = transaction
        .find("install_keychain_provider")
        .expect("install transaction must stage the fixed provider before swapping the app");
    let swap = transaction
        .find("tx.swap()")
        .expect("install transaction must retain its existing atomic swap");

    assert!(
        provider < swap,
        "provider mutation must precede the live swap"
    );
    assert!(
        transaction[provider..].contains("rollback_install"),
        "provider failure must enter the same durable rollback path as ASAR/signing failures"
    );
    assert!(
        implementation.contains("Contents/Frameworks/Codex Framework.framework")
            && implementation.contains("@loader_path/IncodexKeyProvider.dylib")
            && implementation.contains("add_load_dylib"),
        "the provider must be installed beside Codex Framework and loaded by normal LC_LOAD_DYLIB"
    );
}

#[test]
fn provider_placement_mutates_only_a_staged_framework_and_rolls_back_local_failure() {
    let home = scratch();
    let app = home.join("ChatGPT.app");
    let version = app.join("Contents/Frameworks/Codex Framework.framework/Versions/Test");
    fs::create_dir_all(&version).unwrap();
    let framework = version.join("Codex Framework");
    let original = synthetic_framework();
    fs::write(&framework, &original).unwrap();
    std::os::unix::fs::symlink("Test", version.parent().unwrap().join("Current")).unwrap();
    std::os::unix::fs::symlink(
        "Versions/Current/Codex Framework",
        app.join("Contents/Frameworks/Codex Framework.framework/Codex Framework"),
    )
    .unwrap();

    install_keychain_provider(&app).unwrap();

    let installed = fs::read(&framework).unwrap();
    assert_eq!(installed.len(), original.len());
    assert_eq!(&installed[0x200..], &original[0x200..]);
    assert_eq!(read_u32(&installed, 16), 4);
    assert!(contains_provider_command(&installed));
    assert_eq!(
        fs::read(version.join("IncodexKeyProvider.dylib")).unwrap(),
        bundled_provider_bytes()
    );

    let broken_app = home.join("Broken.app");
    let broken_dir = broken_app.join("Contents/Frameworks/Codex Framework.framework");
    fs::create_dir_all(&broken_dir).unwrap();
    let broken_framework = broken_dir.join("Codex Framework");
    fs::write(&broken_framework, b"not a Mach-O").unwrap();
    let before = fs::read(&broken_framework).unwrap();

    assert!(install_keychain_provider(&broken_app).is_err());
    assert_eq!(fs::read(&broken_framework).unwrap(), before);
    assert!(!broken_dir.join("IncodexKeyProvider.dylib").exists());

    fs::remove_dir_all(home).unwrap();
}

fn synthetic_framework() -> Vec<u8> {
    const HEADER: usize = 32;
    const SEGMENT: usize = 72;
    const UUID: usize = 24;
    const COMMANDS: usize = SEGMENT * 2 + UUID;
    const CONTENT_OFFSET: usize = 0x200;
    let mut bytes = vec![0; CONTENT_OFFSET + 32];
    write_u32(&mut bytes, 0, 0xfeed_facf);
    write_u32(&mut bytes, 4, 0x0100_000c);
    write_u32(&mut bytes, 12, 6);
    write_u32(&mut bytes, 16, 3);
    write_u32(&mut bytes, 20, COMMANDS as u32);
    write_segment(&mut bytes[HEADER..], b"__TEXT", 0, CONTENT_OFFSET as u64);
    write_segment(
        &mut bytes[HEADER + SEGMENT..],
        b"__DATA",
        CONTENT_OFFSET as u64,
        32,
    );
    write_u32(&mut bytes, HEADER + SEGMENT * 2, 0x1b);
    write_u32(&mut bytes, HEADER + SEGMENT * 2 + 4, UUID as u32);
    bytes[CONTENT_OFFSET..].fill(0xa5);
    bytes
}

fn write_segment(command: &mut [u8], name: &[u8], file_offset: u64, file_size: u64) {
    write_u32(command, 0, 0x19);
    write_u32(command, 4, 72);
    command[8..8 + name.len()].copy_from_slice(name);
    write_u64(command, 40, file_offset);
    write_u64(command, 48, file_size);
}

fn contains_provider_command(bytes: &[u8]) -> bool {
    let mut offset = 32;
    for _ in 0..read_u32(bytes, 16) {
        let size = read_u32(bytes, offset + 4) as usize;
        if read_u32(bytes, offset) == 0x0c {
            let name_offset = read_u32(bytes, offset + 8) as usize;
            let name = &bytes[offset + name_offset..offset + size];
            return name.starts_with(b"@loader_path/IncodexKeyProvider.dylib\0");
        }
        offset += size;
    }
    false
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
