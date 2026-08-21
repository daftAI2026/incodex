use std::env;
use std::fs;
use std::io::Write;
use std::os::unix::fs::symlink;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::Command;

use incodex_asar::{pack_dir, patch_asar, Archive, LOADER_NAME};
use incodex_cli::legacy_migration::{
    migrate_legacy_ts_v1, recover_legacy_ts_v1, recover_legacy_ts_v1_with_checkpoint,
};
use incodex_cli::legacy_proof::prove_legacy_ts_v1;
use incodex_cli::legacy_typescript::load_legacy_ts_v1;
use incodex_macos::{ditto, sign_app};
use incodex_transaction::{journal_v2, tree_digest};
use serde_json::json;
use sha2::{Digest, Sha256};

#[path = "support/legacy_fixture.rs"]
mod legacy_fixture;
use legacy_fixture::{Fixture, INSTALL_ID};

#[test]
fn committed_ts_state_migrates_to_v2_without_using_live_as_original() {
    let fixture = Fixture::create();
    let state = load_legacy_ts_v1(&fixture.root, &fixture.app)
        .unwrap()
        .unwrap();
    let proven = prove_legacy_ts_v1(&fixture.root, state).unwrap();
    let journal = migrate_legacy_ts_v1(&fixture.root, proven).unwrap();
    assert_eq!(journal.phase, "COMMITTED");
    assert_eq!(
        fs::read(fixture.original_app.join("Contents/Resources/app.asar")).unwrap(),
        fixture.original_bytes
    );
    assert_ne!(
        fs::read(fixture.app_asar()).unwrap(),
        fixture.original_bytes
    );
    assert!(fixture.legacy_journal().is_file());
    assert_eq!(
        journal_v2(&fixture.root, INSTALL_ID).unwrap().phase,
        "COMMITTED"
    );
}

#[test]
fn migration_round_trip_restores_exact_original_and_keeps_legacy_record() {
    let fixture = Fixture::create();
    let install = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["install", "--yes", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    let reinstall = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["install", "--yes", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        reinstall.status.success(),
        "{}",
        String::from_utf8_lossy(&reinstall.stderr)
    );
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["uninstall", "--yes", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(fixture.app_asar()).unwrap(),
        fixture.original_bytes
    );
    assert!(Archive::open(fixture.app_asar())
        .unwrap()
        .extract(LOADER_NAME)
        .is_err());
    assert!(fixture.legacy_journal().is_file());
    assert_eq!(
        journal_v2(&fixture.root, INSTALL_ID).unwrap().phase,
        "ROLLED_BACK"
    );
    let second_uninstall = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["uninstall", "--yes", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!second_uninstall.status.success());
    assert!(
        String::from_utf8_lossy(&second_uninstall.stderr).contains("no installation record"),
        "{}",
        String::from_utf8_lossy(&second_uninstall.stderr)
    );
}

#[test]
fn recovery_uses_outgoing_when_legacy_original_is_missing() {
    let fixture = Fixture::create();
    let outgoing = fixture
        .root
        .join("transactions")
        .join(INSTALL_ID)
        .join("outgoing/ChatGPT.app");
    ditto(&fixture.original_app, &outgoing).unwrap();
    fs::remove_dir_all(&fixture.original_app).unwrap();
    fixture.set_phase("SWAPPED");
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["recover", "--transaction", INSTALL_ID])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(fixture.app_asar()).unwrap(),
        fixture.original_bytes
    );
    assert!(!outgoing.exists());
}

#[test]
fn recovery_refuses_to_replace_a_foreign_target_when_original_is_missing() {
    let fixture = Fixture::create();
    let outgoing = fixture
        .root
        .join("transactions")
        .join(INSTALL_ID)
        .join("outgoing/ChatGPT.app");
    ditto(&fixture.original_app, &outgoing).unwrap();
    fs::remove_dir_all(&fixture.original_app).unwrap();
    fs::write(fixture.app_asar(), b"foreign target").unwrap();
    fixture.set_phase("SWAPPED");
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["recover", "--transaction", INSTALL_ID])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(outgoing.exists());
    assert_eq!(fs::read(fixture.app_asar()).unwrap(), b"foreign target");
}

#[test]
fn migration_replaces_an_interrupted_partial_v2_backup() {
    let fixture = Fixture::create();
    let partial = fixture
        .root
        .join("transactions")
        .join(INSTALL_ID)
        .join("original/ChatGPT.app");
    fs::create_dir_all(&partial).unwrap();
    fs::write(partial.join("partial"), b"incomplete").unwrap();
    let state = load_legacy_ts_v1(&fixture.root, &fixture.app)
        .unwrap()
        .unwrap();
    let proven = prove_legacy_ts_v1(&fixture.root, state).unwrap();
    migrate_legacy_ts_v1(&fixture.root, proven).unwrap();
    assert_eq!(
        fs::read(
            fixture
                .root
                .join("transactions")
                .join(INSTALL_ID)
                .join("original/ChatGPT.app/Contents/Resources/app.asar"),
        )
        .unwrap(),
        fixture.original_bytes
    );
}

#[test]
fn recovery_refuses_a_symlinked_legacy_journal_temporary() {
    let fixture = Fixture::create();
    fixture.set_phase("PATCHED");
    let outside = fixture
        .root
        .parent()
        .unwrap()
        .join("legacy-journal-sentinel");
    fs::write(&outside, b"sentinel").unwrap();
    let temporary = fixture.legacy_journal().with_extension("json.tmp");
    symlink(&outside, &temporary).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["recover", "--transaction", INSTALL_ID])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(&outside).unwrap(), b"sentinel");
}

#[test]
fn recovery_verifies_the_restore_candidate_before_replacing_target() {
    let fixture = Fixture::create();
    let patched = fs::read(fixture.app_asar()).unwrap();
    fs::write(
        fixture.original_app.join("Contents/Resources/app.asar"),
        b"damaged backup",
    )
    .unwrap();
    fixture.set_phase("SWAPPED");
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["recover", "--transaction", INSTALL_ID])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(fixture.app_asar()).unwrap(), patched);
}

#[test]
fn recovery_accepts_an_already_restored_target_after_replace_boundary() {
    let fixture = Fixture::create();
    fs::remove_dir_all(&fixture.app).unwrap();
    ditto(&fixture.original_app, &fixture.app).unwrap();
    fixture.set_phase("SWAPPED");
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["recover", "--transaction", INSTALL_ID])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.legacy_journal()).unwrap()).unwrap();
    assert_eq!(journal["phase"], "ROLLED_BACK");
}

#[test]
fn recovery_replaces_a_stale_regular_legacy_journal_temp() {
    let fixture = Fixture::create();
    fixture.set_phase("PATCHED");
    let temporary = fixture.legacy_journal().with_extension("json.tmp");
    fs::write(&temporary, b"stale partial journal").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["recover", "--transaction", INSTALL_ID])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!temporary.exists());
}

#[test]
fn adopted_legacy_record_is_historical_after_an_official_upgrade() {
    let fixture = Fixture::create();
    let state = load_legacy_ts_v1(&fixture.root, &fixture.app)
        .unwrap()
        .unwrap();
    migrate_legacy_ts_v1(
        &fixture.root,
        prove_legacy_ts_v1(&fixture.root, state).unwrap(),
    )
    .unwrap();
    fs::remove_dir_all(&fixture.app).unwrap();
    ditto(&fixture.original_app, &fixture.app).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["install", "--yes", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn outgoing_restore_intent_makes_a_completed_rename_restartable() {
    let fixture = Fixture::create();
    let outgoing = fixture
        .root
        .join("transactions")
        .join(INSTALL_ID)
        .join("outgoing/ChatGPT.app");
    ditto(&fixture.original_app, &outgoing).unwrap();
    let digest = tree_digest(&outgoing).unwrap();
    fs::remove_dir_all(&fixture.original_app).unwrap();
    fs::remove_dir_all(&fixture.app).unwrap();
    ditto(&outgoing, &fixture.app).unwrap();
    fs::remove_dir_all(&outgoing).unwrap();
    fixture.set_phase("SWAPPED");
    let path = fixture.legacy_journal();
    let mut journal: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    journal["recoveryIntent"] = json!("restore-outgoing");
    journal["recoveryDigest"] = json!(digest);
    fs::write(path, serde_json::to_vec_pretty(&journal).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["recover", "--transaction", INSTALL_ID])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn recovery_refuses_a_modified_outgoing_before_replacement() {
    let fixture = Fixture::create();
    let patched = fs::read(fixture.app_asar()).unwrap();
    let outgoing = fixture
        .root
        .join("transactions")
        .join(INSTALL_ID)
        .join("outgoing/ChatGPT.app");
    ditto(&fixture.original_app, &outgoing).unwrap();
    fs::remove_dir_all(&fixture.original_app).unwrap();
    let foreign_source = fixture.root.join("foreign-outgoing-source");
    fs::create_dir_all(&foreign_source).unwrap();
    fs::write(
        foreign_source.join("package.json"),
        "{\"main\":\"index.js\"}\n",
    )
    .unwrap();
    fs::write(foreign_source.join("index.js"), "foreign outgoing\n").unwrap();
    pack_dir(
        &foreign_source,
        &outgoing.join("Contents/Resources/app.asar"),
    )
    .unwrap();
    sign_app(&outgoing).unwrap();
    fixture.set_phase("SWAPPED");
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["recover", "--transaction", INSTALL_ID])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(fixture.app_asar()).unwrap(), patched);
    assert!(outgoing.exists());
}

#[test]
fn recovery_keeps_outgoing_source_when_it_changes_after_restore_intent() {
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
        if checkpoint == "AFTER_RESTORE_INTENT" {
            fs::write(
                outgoing.join("Contents/Info.plist"),
                b"changed after intent",
            )
            .unwrap();
        }
    });

    assert!(result.is_err());
    assert!(outgoing.exists(), "changed source must remain recoverable");
    assert!(
        fixture.app.exists(),
        "live target must not be consumed on proof failure"
    );
}

#[test]
fn recovery_keeps_outgoing_proof_when_target_changes_after_restore_rename() {
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
            fs::write(
                fixture.app.join("Contents/Info.plist"),
                b"changed after rename",
            )
            .unwrap();
        }
    });

    assert!(result.is_err());
    assert!(
        outgoing.exists(),
        "source must remain recoverable after post-rename proof failure"
    );
    assert!(fixture
        .root
        .join("legacy-recovery")
        .join(INSTALL_ID)
        .join("outgoing-proof/ChatGPT.app")
        .exists());
}

#[test]
fn pre_swap_recovery_keeps_outgoing_source_when_it_changes_after_restore_intent() {
    let fixture = Fixture::create();
    let outgoing = fixture
        .root
        .join("transactions")
        .join(INSTALL_ID)
        .join("outgoing/ChatGPT.app");
    ditto(&fixture.original_app, &outgoing).unwrap();
    fs::remove_dir_all(&fixture.app).unwrap();
    fixture.set_phase("PATCHED");

    let result = recover_legacy_ts_v1_with_checkpoint(&fixture.root, INSTALL_ID, |checkpoint| {
        if checkpoint == "AFTER_RESTORE_INTENT" {
            fs::write(
                outgoing.join("Contents/Info.plist"),
                b"changed after intent",
            )
            .unwrap();
        }
    });

    assert!(result.is_err());
    assert!(outgoing.exists(), "changed source must remain recoverable");
    assert!(
        !fixture.app.exists(),
        "target must remain absent on proof failure"
    );
}

#[test]
fn status_reports_actual_legacy_backup_hash_and_incomplete_state() {
    let fixture = Fixture::create();
    let backup_asar = fixture.original_app.join("Contents/Resources/app.asar");
    fs::OpenOptions::new()
        .append(true)
        .open(&backup_asar)
        .unwrap()
        .write_all(b"tampered backup")
        .unwrap();
    let expected = Sha256::digest(fs::read(&backup_asar).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    let report = incodex_cli::diagnose::diagnose_with_root(&fixture.app, &fixture.root);
    let backup = report.backup.expect("legacy backup report");
    assert_eq!(backup["legacy"], true);
    assert_eq!(backup["complete"], false);
    assert_eq!(backup["originalExists"], true);
    assert_eq!(backup["originalAsarFileHash"], expected);
}

#[test]
fn pre_swap_recovery_keeps_outgoing_when_target_reappears_foreign() {
    let fixture = Fixture::create();
    let outgoing = fixture
        .root
        .join("transactions")
        .join(INSTALL_ID)
        .join("outgoing/ChatGPT.app");
    ditto(&fixture.original_app, &outgoing).unwrap();
    let foreign_source = fixture.root.join("foreign-pre-swap-source");
    fs::create_dir_all(&foreign_source).unwrap();
    fs::write(
        foreign_source.join("package.json"),
        "{\"main\":\"index.js\"}\n",
    )
    .unwrap();
    fs::write(foreign_source.join("index.js"), "foreign pre-swap\n").unwrap();
    pack_dir(&foreign_source, &fixture.app_asar()).unwrap();
    patch_asar(&fixture.app_asar(), "legacy-loader\n", Some(INSTALL_ID)).unwrap();
    sign_app(&fixture.app).unwrap();
    fixture.set_phase("PATCHED");
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["recover", "--transaction", INSTALL_ID])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(outgoing.exists());
}

#[test]
fn upgraded_legacy_record_rejects_a_foreign_modified_clean_app() {
    let fixture = Fixture::create();
    let state = load_legacy_ts_v1(&fixture.root, &fixture.app)
        .unwrap()
        .unwrap();
    migrate_legacy_ts_v1(
        &fixture.root,
        prove_legacy_ts_v1(&fixture.root, state).unwrap(),
    )
    .unwrap();
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
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["install", "--yes", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn recovery_rejects_a_symlinked_legacy_recovery_root() {
    let fixture = Fixture::create();
    let outside = fixture
        .root
        .parent()
        .unwrap()
        .join("legacy-recovery-outside");
    fs::create_dir_all(&outside).unwrap();
    let recovery_root = fixture.root.join("legacy-recovery");
    symlink(&outside, &recovery_root).unwrap();
    fixture.set_phase("SWAPPED");
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["recover", "--transaction", INSTALL_ID])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(fs::read_dir(&outside).unwrap().next().is_none());
}

#[test]
fn rust_install_adopts_legacy_state_without_using_patched_live_as_original() {
    let fixture = Fixture::create();
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["install", "--yes", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        journal_v2(&fixture.root, INSTALL_ID).unwrap().phase,
        "COMMITTED"
    );
    assert_ne!(
        fs::read(fixture.app_asar()).unwrap(),
        fixture.original_bytes
    );
    assert_eq!(
        fs::read(
            fixture
                .root
                .join("transactions")
                .join(INSTALL_ID)
                .join("original/ChatGPT.app/Contents/Resources/app.asar")
        )
        .unwrap(),
        fixture.original_bytes
    );
}

#[test]
fn rust_install_refuses_a_modified_legacy_backup_before_writing_v2() {
    let fixture = Fixture::create();
    let backup_asar = fixture.original_app.join("Contents/Resources/app.asar");
    fs::OpenOptions::new()
        .append(true)
        .open(backup_asar)
        .unwrap()
        .write_all(b"tampered")
        .unwrap();
    let live_before = fs::read(fixture.app_asar()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["install", "--yes", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(fixture.app_asar()).unwrap(), live_before);
    assert!(journal_v2(&fixture.root, INSTALL_ID).is_err());
}

#[test]
fn rust_install_refuses_a_live_marker_mismatch_before_writing_v2() {
    let fixture = Fixture::create();
    patch_asar(
        &fixture.app_asar(),
        "legacy-loader\n",
        Some("44444444-4444-4444-8444-444444444444"),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["install", "--yes", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(journal_v2(&fixture.root, INSTALL_ID).is_err());
}

#[test]
fn each_interrupted_ts_phase_recovers_without_silently_ignoring_the_flat_journal() {
    for phase in [
        "DISCOVERED",
        "BACKUP_COMMITTED",
        "STAGED",
        "PATCHED",
        "SIGNED",
        "VERIFIED",
        "TARGET_MOVED_OUT",
        "SWAPPED",
        "TARGET_VERIFIED",
    ] {
        let fixture = Fixture::create();
        fixture.set_phase(phase);
        if phase == "DISCOVERED" {
            let path = fixture.legacy_journal();
            let mut journal: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            journal.as_object_mut().unwrap().remove("outgoingApp");
            fs::write(
                path,
                format!("{}\n", serde_json::to_string_pretty(&journal).unwrap()),
            )
            .unwrap();
        }
        let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
            .args(["recover", "--transaction", INSTALL_ID])
            .env("HOME", fixture.root.parent().unwrap())
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "phase={phase}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let journal: serde_json::Value =
            serde_json::from_slice(&fs::read(fixture.legacy_journal()).unwrap()).unwrap();
        assert_eq!(journal["phase"], "ROLLED_BACK", "phase={phase}");
    }
}

#[test]
fn status_reports_a_committed_legacy_install_instead_of_a_missing_backup() {
    let fixture = Fixture::create();
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["status", "--json", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["backup"]["legacy"], true);
    assert_eq!(report["backup"]["installId"], INSTALL_ID);
}

#[test]
fn legacy_recovery_sigkill_child() {
    let (Some(root), Some(point)) = (
        env::var_os("INCODEX_LEGACY_SIGKILL_ROOT"),
        env::var("INCODEX_LEGACY_SIGKILL_POINT").ok(),
    ) else {
        return;
    };
    let install_id = env::var("INCODEX_LEGACY_SIGKILL_INSTALL_ID").unwrap();
    let _ = recover_legacy_ts_v1_with_checkpoint(Path::new(&root), &install_id, |checkpoint| {
        if checkpoint == point {
            let result = unsafe { libc::kill(libc::getpid(), libc::SIGKILL) };
            assert_eq!(result, 0);
            loop {
                std::thread::park();
            }
        }
    });
}

#[test]
fn legacy_recovery_survives_real_subprocess_sigkill_boundaries() {
    for point in [
        "AFTER_RESTORE_INTENT",
        "AFTER_RESTORE_RENAME",
        "AFTER_RECOVERY_CLEANUP",
        "BEFORE_ROLLED_BACK_JOURNAL",
        "BEFORE_LEGACY_INTENT_JOURNAL_RENAME",
        "BEFORE_LEGACY_ROLLED_BACK_JOURNAL_RENAME",
    ] {
        let fixture = Fixture::create();
        let outgoing = fixture
            .root
            .join("transactions")
            .join(INSTALL_ID)
            .join("outgoing/ChatGPT.app");
        ditto(&fixture.original_app, &outgoing).unwrap();
        fs::remove_dir_all(&fixture.original_app).unwrap();
        fixture.set_phase("SWAPPED");
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "legacy_recovery_sigkill_child", "--nocapture"])
            .env("INCODEX_LEGACY_SIGKILL_ROOT", &fixture.root)
            .env("INCODEX_LEGACY_SIGKILL_INSTALL_ID", INSTALL_ID)
            .env("INCODEX_LEGACY_SIGKILL_POINT", point)
            .output()
            .unwrap();
        assert_eq!(child.status.signal(), Some(libc::SIGKILL), "point={point}");
        let recovered = recover_legacy_ts_v1(&fixture.root, INSTALL_ID).unwrap();
        assert_eq!(recovered.phase, "ROLLED_BACK", "point={point}");
        let recovered_again = recover_legacy_ts_v1(&fixture.root, INSTALL_ID).unwrap();
        assert_eq!(recovered_again.action, "done", "point={point}");
        assert_eq!(
            fs::read(fixture.app_asar()).unwrap(),
            fixture.original_bytes
        );
    }
}
