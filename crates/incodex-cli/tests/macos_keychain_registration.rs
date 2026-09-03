#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::macos_keychain_assets::{ensure_registration, read_registration};
use incodex_cli::macos_update_restore::{
    publish_registration as publish_update_registration, refresh_registered_helper,
};
use sha2::{Digest, Sha256};

static SEQ: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    home: PathBuf,
    root: PathBuf,
    app: PathBuf,
    keychain_helper_v1: PathBuf,
    keychain_helper_v2: PathBuf,
    update_helper_v1: PathBuf,
    update_helper_v2: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = SEQ.fetch_add(1, Ordering::Relaxed);
        let home = std::env::temp_dir().join(format!(
            "incodex-macos-keychain-registration-{}-{sequence}",
            std::process::id()
        ));
        let root = home.join(".incodex");
        let app = home.join("Applications/ChatGPT.app");
        fs::create_dir_all(&app).unwrap();

        let keychain_helper_v1 =
            write_source(&home, "keychain-helper-v1", b"stable keychain helper\n");
        let keychain_helper_v2 = write_source(
            &home,
            "keychain-helper-v2",
            b"replacement keychain helper\n",
        );
        let update_helper_v1 = write_source(&home, "update-helper-v1", b"update helper v1\n");
        let update_helper_v2 = write_source(&home, "update-helper-v2", b"update helper v2\n");

        Self {
            home,
            root,
            app,
            keychain_helper_v1,
            keychain_helper_v2,
            update_helper_v1,
            update_helper_v2,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

fn write_source(home: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = home.join(name);
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn keychain_helper_identity_survives_host_reinstall_runtime_refresh_and_update_recovery() {
    let fixture = Fixture::new();

    // First install publishes both state machines. They must never share a generation.
    let update_before = publish_update_registration(
        &fixture.root,
        &fixture.update_helper_v1,
        &fixture.app,
        "install-epoch-a",
    )
    .unwrap();
    let keychain_before =
        ensure_registration(&fixture.root, &fixture.app, &fixture.keychain_helper_v1).unwrap();

    // A Runtime refresh must not rotate the long-lived Keychain Helper.
    let keychain_after_runtime =
        ensure_registration(&fixture.root, &fixture.app, &fixture.keychain_helper_v1).unwrap();

    // An official update moves only the content-addressed Update Helper generation.
    assert!(refresh_registered_helper(&fixture.root, &fixture.update_helper_v2).unwrap());
    let update_after = incodex_cli::macos_update_restore::read_registration(&fixture.root)
        .unwrap()
        .unwrap();

    // Repatching the replacement host must resolve the same Keychain Helper.
    let keychain_after_update =
        ensure_registration(&fixture.root, &fixture.app, &fixture.keychain_helper_v1).unwrap();

    assert_eq!(
        keychain_before.helper_path,
        keychain_after_runtime.helper_path
    );
    assert_eq!(
        keychain_before.helper_sha256,
        keychain_after_runtime.helper_sha256
    );
    assert_eq!(
        keychain_before.helper_path,
        keychain_after_update.helper_path
    );
    assert_eq!(
        keychain_before.helper_sha256,
        keychain_after_update.helper_sha256
    );
    assert_eq!(keychain_after_update.schema_version, 1);
    assert_eq!(keychain_after_update.app_path, fixture.app);
    assert_eq!(
        keychain_after_update.helper_sha256,
        sha256_hex(b"stable keychain helper\n")
    );
    assert_eq!(
        fs::read(&keychain_after_update.helper_path).unwrap(),
        b"stable keychain helper\n"
    );

    assert!(keychain_after_update
        .helper_path
        .starts_with(fixture.root.join("helpers/macos-keychain")));
    assert!(update_after
        .helper_path
        .starts_with(fixture.root.join("helpers/macos-update")));
    assert_ne!(keychain_after_update.helper_path, update_after.helper_path);
    assert_ne!(update_before.helper_sha256, update_after.helper_sha256);

    let persisted = read_registration(&fixture.root).unwrap().unwrap();
    assert_eq!(persisted, keychain_after_update);
    assert_eq!(
        fs::metadata(&keychain_after_update.helper_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(fixture.root.join("macos-keychain/registration.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn keychain_helper_source_change_never_silently_replaces_the_authorized_identity() {
    let fixture = Fixture::new();
    let mut original =
        ensure_registration(&fixture.root, &fixture.app, &fixture.keychain_helper_v1).unwrap();
    original.authorization_ready = true;
    let registration_path = fixture.root.join("macos-keychain/registration.json");
    fs::write(
        &registration_path,
        format!("{}\n", serde_json::to_string(&original).unwrap()),
    )
    .unwrap();
    fs::set_permissions(&registration_path, fs::Permissions::from_mode(0o600)).unwrap();

    // A future Helper binary may be offered for an explicit migration, but an ordinary
    // Runtime/host refresh must not silently replace the identity already authorized by macOS.
    let observed =
        ensure_registration(&fixture.root, &fixture.app, &fixture.keychain_helper_v2).unwrap();
    assert_eq!(observed, original);
    assert_eq!(
        fs::read(&observed.helper_path).unwrap(),
        b"stable keychain helper\n"
    );

    assert_eq!(read_registration(&fixture.root).unwrap().unwrap(), original);
}

#[test]
fn unready_keychain_helper_migrates_before_its_first_successful_authorization() {
    let fixture = Fixture::new();
    let original =
        ensure_registration(&fixture.root, &fixture.app, &fixture.keychain_helper_v1).unwrap();
    assert!(!original.authorization_ready);

    let migrated =
        ensure_registration(&fixture.root, &fixture.app, &fixture.keychain_helper_v2).unwrap();

    assert_ne!(migrated.helper_sha256, original.helper_sha256);
    assert_eq!(
        migrated.helper_sha256,
        sha256_hex(b"replacement keychain helper\n")
    );
    assert_eq!(
        fs::read(&migrated.helper_path).unwrap(),
        b"replacement keychain helper\n"
    );
    assert!(!migrated.authorization_ready);
    assert_eq!(read_registration(&fixture.root).unwrap().unwrap(), migrated);
}

#[test]
fn keychain_registration_rejects_a_symlinked_control_file() {
    let fixture = Fixture::new();
    let registration =
        ensure_registration(&fixture.root, &fixture.app, &fixture.keychain_helper_v1).unwrap();
    let registration_path = fixture.root.join("macos-keychain/registration.json");
    let foreign = fixture.home.join("foreign-registration.json");
    fs::write(&foreign, b"{}\n").unwrap();
    fs::remove_file(&registration_path).unwrap();
    std::os::unix::fs::symlink(&foreign, &registration_path).unwrap();

    let error = read_registration(&fixture.root).unwrap_err();
    assert!(error.contains("symlink"), "{error}");
    assert_eq!(
        registration.helper_sha256,
        sha256_hex(b"stable keychain helper\n")
    );
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
