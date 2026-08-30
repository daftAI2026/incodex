#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use incodex_cli::macos_update_restore::{
    publish_registration, read_registration, refresh_registered_helper, remove_registration,
};

fn scratch() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "incodex-macos-update-registration-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn helper_is_content_addressed_private_and_bound_to_install_epoch() {
    let home = scratch();
    let root = home.join(".incodex");
    let source = home.join("incodex-source");
    fs::write(&source, b"native helper fixture").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();
    let app = Path::new("/Applications/ChatGPT.app");

    let registration = publish_registration(&root, &source, app, "install-epoch-a").unwrap();

    assert_eq!(registration.schema_version, 1);
    assert_eq!(registration.app_path, app);
    assert_eq!(registration.install_id, "install-epoch-a");
    assert!(registration
        .helper_path
        .starts_with(root.join("helpers/macos-update")));
    assert_eq!(registration.helper_sha256.len(), 64);
    assert_eq!(
        fs::read(&registration.helper_path).unwrap(),
        b"native helper fixture"
    );
    assert_eq!(
        fs::metadata(&registration.helper_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(registration.helper_path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );

    let persisted = read_registration(&root).unwrap().unwrap();
    assert_eq!(persisted, registration);
    assert_eq!(
        fs::metadata(root.join("macos-update/registration.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    remove_registration(&root, "install-epoch-a").unwrap();
    assert!(read_registration(&root).unwrap().is_none());
}

#[test]
fn stale_worker_cannot_remove_a_new_registration() {
    let home = scratch();
    let root = home.join(".incodex");
    let source = home.join("incodex-source");
    fs::write(&source, b"native helper fixture").unwrap();
    let app = Path::new("/Applications/ChatGPT.app");

    publish_registration(&root, &source, app, "install-epoch-b").unwrap();
    remove_registration(&root, "install-epoch-a").unwrap();

    assert_eq!(
        read_registration(&root).unwrap().unwrap().install_id,
        "install-epoch-b"
    );
}

#[test]
fn runtime_update_refreshes_the_helper_without_changing_the_install_epoch() {
    let home = scratch();
    let root = home.join(".incodex");
    let old_source = home.join("incodex-old");
    let new_source = home.join("incodex-new");
    fs::write(&old_source, b"old helper fixture").unwrap();
    fs::write(&new_source, b"new helper fixture").unwrap();
    let app = Path::new("/Applications/ChatGPT.app");
    let original = publish_registration(&root, &old_source, app, "install-epoch-a").unwrap();

    assert!(refresh_registered_helper(&root, &new_source).unwrap());

    let refreshed = read_registration(&root).unwrap().unwrap();
    assert_eq!(refreshed.install_id, original.install_id);
    assert_eq!(refreshed.app_path, original.app_path);
    assert_ne!(refreshed.helper_sha256, original.helper_sha256);
    assert_eq!(
        fs::read(refreshed.helper_path).unwrap(),
        b"new helper fixture"
    );
}

#[test]
fn registration_reader_rejects_a_symlinked_control_file() {
    let home = scratch();
    let root = home.join(".incodex");
    let state = root.join("macos-update");
    fs::create_dir_all(&state).unwrap();
    let foreign = home.join("foreign.json");
    fs::write(&foreign, b"{}\n").unwrap();
    std::os::unix::fs::symlink(&foreign, state.join("registration.json")).unwrap();

    let error = read_registration(&root).unwrap_err();
    assert!(error.contains("symlink"), "{error}");
}
