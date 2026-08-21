use std::fs;
use std::io::Write;

use incodex_cli::legacy_migration::{recover_legacy_ts_v1, recover_legacy_ts_v1_with_checkpoint};
use incodex_macos::ditto;

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
