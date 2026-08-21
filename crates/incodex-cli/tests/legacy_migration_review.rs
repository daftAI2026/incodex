use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;

use incodex_asar::pack_dir;
use incodex_cli::legacy_migration::{
    migrate_legacy_if_needed, recover_legacy_ts_v1, recover_legacy_ts_v1_with_checkpoint,
};
use incodex_macos::{ditto, sign_app};
use incodex_transaction::tree_digest;

#[path = "support/legacy_fixture.rs"]
mod legacy_fixture;
use legacy_fixture::{Fixture, INSTALL_ID};

#[test]
fn recovery_reconciles_a_mutated_outgoing_with_the_sealed_proof_on_retry() {
    let fixture = Fixture::create();
    let outgoing = fixture
        .root
        .join("transactions")
        .join(INSTALL_ID)
        .join("outgoing/ChatGPT.app");
    ditto(&fixture.original_app, &outgoing).unwrap();
    fs::remove_dir_all(&fixture.original_app).unwrap();
    fixture.set_phase("SWAPPED");

    let first = recover_legacy_ts_v1_with_checkpoint(&fixture.root, INSTALL_ID, |checkpoint| {
        if checkpoint == "AFTER_RESTORE_RENAME" {
            fs::write(
                fixture.app.join("Contents/Info.plist"),
                b"changed after rename",
            )
            .unwrap();
        }
    });
    assert!(first.is_err());
    assert!(outgoing.exists());

    let second = recover_legacy_ts_v1(&fixture.root, INSTALL_ID);
    assert!(
        second.is_ok(),
        "sealed proof should win on retry: {second:?}"
    );
    assert_eq!(
        fs::read(fixture.app_asar()).unwrap(),
        fixture.original_bytes
    );
    assert!(!outgoing.exists());
    assert!(!fixture
        .root
        .join("legacy-recovery")
        .join(INSTALL_ID)
        .join("outgoing-proof/ChatGPT.app")
        .exists());
}

#[test]
fn recovery_rejects_an_extra_file_added_after_the_outgoing_rename() {
    let fixture = Fixture::create();
    let outgoing = fixture
        .root
        .join("transactions")
        .join(INSTALL_ID)
        .join("outgoing/ChatGPT.app");
    ditto(&fixture.original_app, &outgoing).unwrap();
    fs::remove_dir_all(&fixture.original_app).unwrap();
    fixture.set_phase("SWAPPED");

    let result = recover_legacy_ts_v1_with_checkpoint(&fixture.root, INSTALL_ID, |checkpoint| {
        if checkpoint == "AFTER_RESTORE_RENAME" {
            let plist = fixture.app.join("Contents/Info.plist");
            let mut permissions = fs::metadata(&plist).unwrap().permissions();
            permissions.set_mode(0o600);
            fs::set_permissions(plist, permissions).unwrap();
        }
    });

    assert!(result.is_err());
    assert!(
        outgoing.exists(),
        "post-rename tree mismatch must retain source"
    );
}

#[test]
fn recovery_rejects_patched_target_tree_drift_when_staged_is_absent() {
    let fixture = Fixture::create();
    fixture.set_phase("SWAPPED");
    let plist = fixture.app.join("Contents/Info.plist");
    let mut permissions = fs::metadata(&plist).unwrap().permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(plist, permissions).unwrap();

    let result = recover_legacy_ts_v1(&fixture.root, INSTALL_ID);
    assert!(result.is_err(), "staged-less recovery must prove the full target tree");
    assert!(fixture.app.exists(), "failed proof must not replace the target");
}

#[test]
fn status_rejects_a_legacy_backup_with_a_modified_executable() {
    let fixture = Fixture::create();
    fs::OpenOptions::new()
        .append(true)
        .open(fixture.original_app.join("Contents/MacOS/ChatGPT"))
        .unwrap()
        .write_all(b"tampered executable")
        .unwrap();

    let report = incodex_cli::diagnose::diagnose_with_root(&fixture.app, &fixture.root);
    assert_eq!(report.backup.unwrap()["complete"], false);
}

#[test]
fn status_rejects_a_re_signed_legacy_backup_with_non_asar_tree_drift() {
    let fixture = Fixture::create();
    let target_dir = fs::read_dir(fixture.root.join("installations"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let manifest_path = target_dir.join(INSTALL_ID).join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["originalTreeDigest"] = serde_json::json!(tree_digest(&fixture.original_app).unwrap());
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::write(
        fixture.original_app.join("Contents/Resources/foreign.txt"),
        "foreign member",
    )
    .unwrap();
    sign_app(&fixture.original_app).unwrap();

    let report = incodex_cli::diagnose::diagnose_with_root(&fixture.app, &fixture.root);
    assert_eq!(report.backup.unwrap()["complete"], false);
}

#[test]
fn unadopted_legacy_record_becomes_historical_after_clean_vendor_upgrade() {
    let fixture = Fixture::create();
    ditto(&fixture.original_app, &fixture.app).unwrap();

    let result = migrate_legacy_if_needed(&fixture.root, &fixture.app);
    assert_eq!(result.unwrap(), None);
}

#[test]
fn unadopted_legacy_record_rejects_a_foreign_modified_app() {
    let fixture = Fixture::create();
    let foreign_source = fixture.root.join("foreign-clean-source");
    fs::create_dir_all(&foreign_source).unwrap();
    fs::write(
        foreign_source.join("package.json"),
        "{\"main\":\"index.js\"}\n",
    )
    .unwrap();
    fs::write(foreign_source.join("index.js"), "foreign modified\n").unwrap();
    pack_dir(&foreign_source, &fixture.app_asar()).unwrap();
    sign_app(&fixture.app).unwrap();

    let result = migrate_legacy_if_needed(&fixture.root, &fixture.app);
    assert!(result.is_err(), "foreign app must fail closed: {result:?}");
}
