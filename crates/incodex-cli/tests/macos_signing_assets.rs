#![cfg(target_os = "macos")]

/**
 * [INPUT]: 依赖 CLI 稳定签名身份注册 API，只在临时 Incodex root 创建合成身份
 * [OUTPUT]: 证明首次创建、只读读取、损坏拒绝、私有权限与并发不轮换合同
 * [POS]: macOS signing identity backend 的集成回归，不触碰宿主、真实 Keychain、信任设置或 TCC
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use incodex_cli::macos_signing_assets::{
    ensure_signing_identity, read_signing_identity, unlock_signing_identity,
};
use serde_json::Value;

static SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    home: PathBuf,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let home = std::env::temp_dir().join(format!(
            "incodex-macos-signing-assets-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            sequence,
        ));
        Self {
            root: home.join(".incodex"),
            home,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

#[test]
fn read_is_pure_and_only_explicit_ensure_creates_the_stable_identity() {
    let fixture = Fixture::new();
    assert!(!fixture.root.exists());

    assert_eq!(read_signing_identity(&fixture.root).unwrap(), None);
    assert!(!fixture.root.exists(), "read must not create Incodex state");

    let first = ensure_signing_identity(&fixture.root).unwrap();
    let second = ensure_signing_identity(&fixture.root).unwrap();
    assert_eq!(first, second, "ensure must reuse the first local identity");
    assert_eq!(
        read_signing_identity(&fixture.root).unwrap(),
        Some(first.clone())
    );

    let identity_dir = fixture.root.join("macos-signing");
    assert!(first.keychain_path.starts_with(&identity_dir));
    assert_eq!(
        fs::metadata(&identity_dir).unwrap().permissions().mode() & 0o777,
        0o700,
        "identity material must stay inside a private directory"
    );
    assert_regular_private_file(&first.keychain_path);
    assert_eq!(
        fs::metadata(identity_dir.join("identity.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "registration metadata must not be publicly writable"
    );
    assert_fingerprint(&first.certificate_sha1, 40);
    assert_fingerprint(&first.certificate_sha256, 64);
}

#[test]
fn registered_metadata_is_public_and_contains_no_private_key_or_password() {
    let fixture = Fixture::new();
    let identity = ensure_signing_identity(&fixture.root).unwrap();
    let registration_path = fixture.root.join("macos-signing/identity.json");
    let bytes = fs::read(&registration_path).unwrap();
    let metadata: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(metadata["certificateSha1"], identity.certificate_sha1);
    assert_eq!(metadata["certificateSha256"], identity.certificate_sha256);
    assert_eq!(
        metadata["keychainPath"],
        identity.keychain_path.display().to_string()
    );

    let encoded = String::from_utf8(bytes).unwrap().to_ascii_lowercase();
    for forbidden in ["password", "privatekey", "pkcs12", "-----begin", "secret"] {
        assert!(
            !encoded.contains(forbidden),
            "public identity metadata must not contain {forbidden}"
        );
    }
}

#[test]
fn read_and_ensure_fail_closed_when_registered_keychain_is_missing() {
    let fixture = Fixture::new();
    let identity = ensure_signing_identity(&fixture.root).unwrap();
    let registration_path = fixture.root.join("macos-signing/identity.json");
    let registration_before = fs::read(&registration_path).unwrap();
    fs::remove_file(&identity.keychain_path).unwrap();

    assert!(read_signing_identity(&fixture.root).is_err());
    assert!(ensure_signing_identity(&fixture.root).is_err());
    assert!(
        !identity.keychain_path.exists(),
        "ensure must not silently regenerate"
    );
    assert_eq!(fs::read(registration_path).unwrap(), registration_before);
}

#[test]
fn read_and_ensure_reject_non_keychain_bytes_even_when_registration_is_intact() {
    let fixture = Fixture::new();
    let identity = ensure_signing_identity(&fixture.root).unwrap();
    let registration_path = fixture.root.join("macos-signing/identity.json");
    let registration_before = fs::read(&registration_path).unwrap();
    fs::write(&identity.keychain_path, b"synthetic-not-a-keychain").unwrap();

    assert!(
        read_signing_identity(&fixture.root).is_err(),
        "private Keychain bytes, not only certificate metadata, must be verified"
    );
    assert!(ensure_signing_identity(&fixture.root).is_err());
    assert_eq!(
        fs::read(&identity.keychain_path).unwrap(),
        b"synthetic-not-a-keychain",
        "a damaged registered identity must not be regenerated"
    );
    assert_eq!(fs::read(registration_path).unwrap(), registration_before);
}

#[test]
fn read_rejects_a_different_synthetic_identity_keychain() {
    let first = Fixture::new();
    let second = Fixture::new();
    let first_identity = ensure_signing_identity(&first.root).unwrap();
    let second_identity = ensure_signing_identity(&second.root).unwrap();
    assert_ne!(
        first_identity.certificate_sha256,
        second_identity.certificate_sha256
    );
    fs::copy(&second_identity.keychain_path, &first_identity.keychain_path).unwrap();

    assert!(
        read_signing_identity(&first.root).is_err(),
        "a keychain holding another certificate cannot satisfy this registration"
    );
    assert!(ensure_signing_identity(&first.root).is_err());
}

#[test]
fn malformed_or_symlinked_registration_is_never_replaced() {
    let fixture = Fixture::new();
    let _identity = ensure_signing_identity(&fixture.root).unwrap();
    let state = fixture.root.join("macos-signing");
    let registration = state.join("identity.json");
    let foreign = fixture.home.join("foreign-identity.json");
    fs::write(&foreign, b"{\"foreign\":true}\n").unwrap();

    fs::write(&registration, b"{ malformed\n").unwrap();
    assert!(read_signing_identity(&fixture.root).is_err());
    assert!(ensure_signing_identity(&fixture.root).is_err());
    assert_eq!(fs::read(&registration).unwrap(), b"{ malformed\n");

    fs::remove_file(&registration).unwrap();
    symlink(&foreign, &registration).unwrap();
    assert!(read_signing_identity(&fixture.root).is_err());
    assert!(ensure_signing_identity(&fixture.root).is_err());
    assert_eq!(fs::read(&foreign).unwrap(), b"{\"foreign\":true}\n");
}

#[test]
fn partial_identity_state_is_not_treated_as_a_fresh_install() {
    let fixture = Fixture::new();
    let state = fixture.root.join("macos-signing");
    fs::create_dir_all(&state).unwrap();
    let marker = state.join("unexpected-partial-state");
    fs::write(&marker, b"interrupted identity generation").unwrap();

    assert!(ensure_signing_identity(&fixture.root).is_err());
    assert!(!state.join("identity.json").exists());
    assert_eq!(
        fs::read(marker).unwrap(),
        b"interrupted identity generation"
    );
}

#[test]
fn concurrent_first_installs_never_publish_two_identities() {
    let fixture = Fixture::new();
    let root = Arc::new(fixture.root.clone());
    let barrier = Arc::new(Barrier::new(4));
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let root = Arc::clone(&root);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                ensure_signing_identity(&root)
            })
        })
        .collect();

    let mut identities = Vec::new();
    let mut contended = Vec::new();
    for worker in workers {
        match worker.join().unwrap() {
            Ok(identity) => identities.push(identity),
            Err(error) => {
                assert!(
                    error.contains("another incodex command is modifying this app"),
                    "unexpected initialization failure: {error}"
                );
                contended.push(error);
            }
        }
    }
    assert!(!identities.is_empty(), "one first-install caller must win");
    let registered = read_signing_identity(&fixture.root)
        .unwrap()
        .expect("one first-install caller must publish the identity");
    identities.push(registered.clone());

    for identity in identities {
        assert_eq!(identity, registered);
    }
    // The transaction lock is fail-fast; after the winner exits, retries reuse its identity.
    for _ in contended {
        assert_eq!(ensure_signing_identity(&fixture.root).unwrap(), registered);
    }
    assert_eq!(ensure_signing_identity(&fixture.root).unwrap(), registered);
}

#[test]
fn unlock_uses_the_registered_identity_without_changing_the_user_search_list() {
    let fixture = Fixture::new();
    let identity = ensure_signing_identity(&fixture.root).unwrap();
    let before = user_keychain_search_list();

    unlock_signing_identity(&fixture.root, &identity).unwrap();

    assert_eq!(user_keychain_search_list(), before);
    assert_eq!(
        read_signing_identity(&fixture.root).unwrap(),
        Some(identity)
    );
}

fn assert_fingerprint(value: &str, length: usize) {
    assert_eq!(value.len(), length);
    assert!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "fingerprints must be canonical lowercase hex"
    );
}

fn assert_regular_private_file(path: &Path) {
    let metadata = fs::symlink_metadata(path).unwrap();
    assert!(
        metadata.file_type().is_file(),
        "identity asset must not be a symlink"
    );
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
}

fn user_keychain_search_list() -> String {
    let output = std::process::Command::new("/usr/bin/security")
        .args(["list-keychains", "-d", "user"])
        .output()
        .expect("macOS security tool must be available");
    assert!(
        output.status.success(),
        "could not read the current user's keychain search list"
    );
    String::from_utf8(output.stdout).unwrap()
}
