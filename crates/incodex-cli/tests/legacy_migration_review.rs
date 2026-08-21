use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use incodex_asar::pack_dir;
use incodex_cli::legacy_migration::{
    migrate_legacy_if_needed, recover_legacy_ts_v1, recover_legacy_ts_v1_with_checkpoint,
};
use incodex_macos::{ditto, read_asar_integrity, sign_app};

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
fn recovery_accepts_the_real_ts_integrity_plist_rewrite_when_staged_is_absent() {
    let fixture = Fixture::create();
    fixture.set_phase("SWAPPED");

    let result = recover_legacy_ts_v1(&fixture.root, INSTALL_ID);
    assert!(result.is_ok(), "real TS integrity rewrite is allowed: {result:?}");
}

#[test]
fn recovery_does_not_depend_on_python3_in_path() {
    let fixture = Fixture::create();
    fixture.set_phase("SWAPPED");
    let fake_bin = fixture.root.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let python = fake_bin.join("python3");
    fs::write(&python, "#!/bin/sh\nexit 127\n").unwrap();
    let mut permissions = fs::metadata(&python).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&python, permissions).unwrap();
    let path = format!(
        "{}:/usr/bin:/bin:/usr/sbin:/sbin",
        fake_bin.display()
    );

    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["recover", "--transaction", INSTALL_ID])
        .env("HOME", fixture.root.parent().unwrap())
        .env("PATH", path)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "plutil-based proof must not require python3: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn recovery_rejects_foreign_electron_integrity_entries_after_resign() {
    let fixture = Fixture::create();
    fixture.set_phase("SWAPPED");
    let plist = fixture.app.join("Contents/Info.plist");
    let patched_hash = read_asar_integrity(&fixture.app).unwrap();
    let integrity = serde_json::json!({
        "Resources/app.asar": {"algorithm": "SHA256", "hash": patched_hash},
        "Other.app": {"algorithm": "SHA256", "hash": "foreign"}
    });
    let status = Command::new("plutil")
        .args([
            "-replace",
            "ElectronAsarIntegrity",
            "-json",
            &serde_json::to_string(&integrity).unwrap(),
            "--",
        ])
        .arg(&plist)
        .status()
        .unwrap();
    assert!(status.success());
    sign_app(&fixture.app).unwrap();

    let result = recover_legacy_ts_v1(&fixture.root, INSTALL_ID);
    assert!(
        result.is_err(),
        "unrelated ElectronAsarIntegrity entries must not be normalized away"
    );
}

#[test]
fn recovery_rejects_a_wrong_app_asar_integrity_hash_after_resign() {
    let fixture = Fixture::create();
    fixture.set_phase("SWAPPED");
    let plist = fixture.app.join("Contents/Info.plist");
    let integrity = serde_json::json!({
        "Resources/app.asar": {"algorithm": "SHA256", "hash": "wrong"}
    });
    let status = Command::new("plutil")
        .args([
            "-replace",
            "ElectronAsarIntegrity",
            "-json",
            &serde_json::to_string(&integrity).unwrap(),
            "--",
        ])
        .arg(&plist)
        .status()
        .unwrap();
    assert!(status.success());
    sign_app(&fixture.app).unwrap();

    let result = recover_legacy_ts_v1(&fixture.root, INSTALL_ID);
    assert!(result.is_err(), "wrong app.asar integrity hash must fail closed");
}

#[test]
fn recovery_rejects_a_modified_foreign_integrity_entry() {
    let fixture = Fixture::create();
    fixture.set_phase("SWAPPED");
    let target_plist = fixture.app.join("Contents/Info.plist");
    let original_plist = fixture.original_app.join("Contents/Info.plist");
    let patched_hash = read_asar_integrity(&fixture.app).unwrap();
    let original_integrity = serde_json::json!({
        "Resources/app.asar": {"algorithm": "SHA256", "hash": "original"},
        "Other.app": {"algorithm": "SHA256", "hash": "original-foreign"}
    });
    let target_integrity = serde_json::json!({
        "Resources/app.asar": {"algorithm": "SHA256", "hash": patched_hash},
        "Other.app": {"algorithm": "SHA256", "hash": "modified-foreign"}
    });
    for (path, integrity) in [(&original_plist, original_integrity), (&target_plist, target_integrity)] {
        let status = Command::new("plutil")
            .args([
                "-replace",
                "ElectronAsarIntegrity",
                "-json",
                &serde_json::to_string(&integrity).unwrap(),
                "--",
            ])
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
    }
    sign_app(&fixture.app).unwrap();

    let result = recover_legacy_ts_v1(&fixture.root, INSTALL_ID);
    assert!(result.is_err(), "foreign integrity drift must fail closed");
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
fn status_does_not_require_a_nonexistent_legacy_tree_seal() {
    let fixture = Fixture::create();
    let report = incodex_cli::diagnose::diagnose_with_root(&fixture.app, &fixture.root);
    let backup = report.backup.unwrap();
    assert_eq!(backup["complete"], true);
    assert_eq!(backup["originalTreeProof"], "not recorded by TS v1");
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
