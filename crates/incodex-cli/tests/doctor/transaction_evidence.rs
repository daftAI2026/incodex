use super::*;
use incodex_transaction::JournalV2;
use std::os::unix::fs::symlink;

#[test]
fn status_skips_transaction_original_proof_but_doctor_checks_it() {
    let home = isolated_home();
    let root = home.join(".incodex");
    let app = home.join("ChatGPT.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "original\n").unwrap();

    let mut tx = Engine::begin(&root, &app, "status-proof-test").unwrap();
    let id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    ditto(&app, &original).unwrap();
    tx.mark_backup_committed().unwrap();
    drop(tx);

    let app_arg = app.to_str().unwrap();
    let (_status, status_stdout, status_stderr) =
        run(&["status", "--json", "--app", app_arg], &home);
    assert_eq!(status_stderr, "");
    let status_report = parse_json(&status_stdout);
    let status_record = status_report["journalRecords"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["installId"] == id)
        .expect("status transaction record");
    assert_eq!(status_record["retainedOriginal"], original.display().to_string());
    assert!(status_record["originalValid"].is_null());

    let (_status, status_human, status_stderr) = run(&["status", "--app", app_arg], &home);
    assert_eq!(status_stderr, "");
    assert!(!status_human.contains("Original proof"), "{status_human}");

    let (_status, doctor_stdout, doctor_stderr) =
        run(&["doctor", "--json", "--app", app_arg], &home);
    assert_eq!(doctor_stderr, "");
    let doctor_report = parse_json(&doctor_stdout);
    let doctor_record = doctor_report["journalRecords"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["installId"] == id)
        .expect("doctor transaction record");
    assert_eq!(doctor_record["originalValid"], true);
}

#[test]
fn doctor_json_reports_retained_original_and_artifacts_for_interrupted_install() {
    let home = isolated_home();
    let root = home.join(".incodex");
    let app = home.join("ChatGPT.app");
    let candidate = home.join("candidate.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "original\n").unwrap();
    fs::create_dir_all(&candidate).unwrap();
    fs::write(candidate.join("marker"), "staged\n").unwrap();

    let mut tx = Engine::begin(&root, &app, "test-interrupted-install").unwrap();
    let id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    ditto(&app, &original).unwrap();
    tx.mark_backup_committed().unwrap();
    tx.place_staging(&candidate).unwrap();
    drop(tx);

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    let record = report["journalRecords"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["installId"] == id && record["kind"] == "validInterrupted")
        .expect("interrupted transaction record");
    assert_eq!(
        record["retainedOriginal"],
        original.display().to_string()
    );
    assert_eq!(record["originalValid"], true);
    assert_eq!(record["recovery"], "rollback");
    assert!(record["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path.as_str().unwrap().ends_with("staging/ChatGPT.app")));

    let (_status, human, stderr) = run(&["doctor", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    assert!(human.contains("Retained original"), "{human}");
    assert!(human.contains("Artifact"), "{human}");
}

#[test]
fn doctor_marks_rolled_back_external_staging_manual() {
    let home = isolated_home();
    let root = home.join(".incodex");
    let app = home.join("ChatGPT.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "original\n").unwrap();

    let mut tx = Engine::begin(&root, &app, "rolled-back-artifact-test").unwrap();
    let id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    ditto(&app, &original).unwrap();
    tx.mark_backup_committed().unwrap();
    tx.rollback("test rollback").unwrap();
    drop(tx);

    let scratch = root
        .join("scratch")
        .join(format!("ChatGPT.app.staged-{id}"));
    fs::create_dir_all(&scratch).unwrap();
    fs::write(scratch.join("marker"), "leftover\n").unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    let record = report["journalRecords"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["installId"] == id)
        .expect("rolled-back transaction record");
    assert_eq!(record["phase"], "ROLLED_BACK");
    assert_eq!(record["recovery"], "manual");
    assert!(record["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path.as_str().unwrap() == scratch.display().to_string()));
}

#[test]
fn doctor_marks_backup_committed_external_staging_manual() {
    let home = isolated_home();
    let root = home.join(".incodex");
    let app = home.join("ChatGPT.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "original\n").unwrap();

    let mut tx = Engine::begin(&root, &app, "backup-committed-artifact-test").unwrap();
    let id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    ditto(&app, &original).unwrap();
    tx.mark_backup_committed().unwrap();
    drop(tx);

    let scratch = root
        .join("scratch")
        .join(format!("ChatGPT.app.staged-{id}"));
    fs::create_dir_all(&scratch).unwrap();
    fs::write(scratch.join("marker"), "leftover\n").unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    let record = report["journalRecords"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["installId"] == id)
        .expect("backup-committed transaction record");
    assert_eq!(record["phase"], "BACKUP_COMMITTED");
    assert_eq!(record["recovery"], "manual");
    assert!(record["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path.as_str().unwrap() == scratch.display().to_string()));
}

#[test]
fn doctor_marks_backup_committed_without_valid_original_manual() {
    let home = isolated_home();
    let root = home.join(".incodex");
    let app = home.join("ChatGPT.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "original\n").unwrap();

    let mut tx = Engine::begin(&root, &app, "missing-backup-artifact-test").unwrap();
    let id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    ditto(&app, &original).unwrap();
    tx.mark_backup_committed().unwrap();
    drop(tx);
    fs::remove_dir_all(&original).unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    let record = report["journalRecords"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["installId"] == id)
        .expect("missing-backup transaction record");
    assert_eq!(record["phase"], "BACKUP_COMMITTED");
    assert!(record["originalValid"].is_null());
    assert_eq!(record["recovery"], "manual");
}

#[test]
fn doctor_marks_committed_external_staging_manual() {
    let home = isolated_home();
    let root = home.join(".incodex");
    let app = home.join("ChatGPT.app");
    let candidate = home.join("candidate.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "original\n").unwrap();
    fs::create_dir_all(&candidate).unwrap();
    fs::write(candidate.join("marker"), "patched\n").unwrap();

    let mut tx = Engine::begin(&root, &app, "committed-artifact-test").unwrap();
    let id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    ditto(&app, &original).unwrap();
    tx.mark_backup_committed().unwrap();
    tx.place_staging(&candidate).unwrap();
    tx.swap().unwrap();
    tx.commit().unwrap();
    drop(tx);

    let scratch = root
        .join("scratch")
        .join(format!("ChatGPT.app.staged-{id}"));
    fs::create_dir_all(&scratch).unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    let record = report["journalRecords"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["installId"] == id)
        .expect("committed transaction record");
    assert_eq!(record["phase"], "COMMITTED");
    assert_eq!(record["recovery"], "manual");
}

#[test]
fn doctor_marks_committed_restore_symlink_manual() {
    let home = isolated_home();
    let root = home.join(".incodex");
    let app = home.join("ChatGPT.app");
    let candidate = home.join("candidate.app");
    let outside = home.join("outside.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "original\n").unwrap();
    fs::create_dir_all(&candidate).unwrap();
    fs::write(candidate.join("marker"), "patched\n").unwrap();
    fs::create_dir_all(&outside).unwrap();

    let mut tx = Engine::begin(&root, &app, "committed-restore-symlink-test").unwrap();
    let id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    ditto(&app, &original).unwrap();
    tx.mark_backup_committed().unwrap();
    tx.place_staging(&candidate).unwrap();
    tx.swap().unwrap();
    tx.commit().unwrap();
    drop(tx);

    let restore = root
        .join("transactions")
        .join(&id)
        .join("restore/ChatGPT.app");
    fs::create_dir_all(restore.parent().unwrap()).unwrap();
    symlink(&outside, &restore).unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    let record = report["journalRecords"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["installId"] == id)
        .expect("committed transaction record");
    assert_eq!(record["phase"], "COMMITTED");
    assert_eq!(record["recovery"], "manual");
    assert!(record["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path.as_str().unwrap() == restore.display().to_string()));
}

#[test]
fn doctor_marks_committed_internal_artifact_cleanup() {
    let home = isolated_home();
    let root = home.join(".incodex");
    let app = home.join("ChatGPT.app");
    let candidate = home.join("candidate.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "original\n").unwrap();
    fs::create_dir_all(&candidate).unwrap();
    fs::write(candidate.join("marker"), "patched\n").unwrap();

    let mut tx = Engine::begin(&root, &app, "committed-internal-artifact-test").unwrap();
    let id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    ditto(&app, &original).unwrap();
    tx.mark_backup_committed().unwrap();
    tx.place_staging(&candidate).unwrap();
    tx.swap().unwrap();
    tx.commit().unwrap();
    drop(tx);

    let staging = root
        .join("transactions")
        .join(&id)
        .join("staging/ChatGPT.app");
    fs::create_dir_all(&staging).unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    let record = report["journalRecords"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["installId"] == id)
        .expect("committed transaction record");
    assert_eq!(record["phase"], "COMMITTED");
    assert_eq!(record["recovery"], "cleanup");
    assert!(record["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path.as_str().unwrap() == staging.display().to_string()));
}

#[test]
fn doctor_marks_a_checksum_valid_unknown_transaction_phase_manual() {
    let home = isolated_home();
    let root = home.join(".incodex");
    let app = home.join("ChatGPT.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "original\n").unwrap();

    let mut tx = Engine::begin(&root, &app, "test-unknown-phase").unwrap();
    let id = tx.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    ditto(&app, &original).unwrap();
    tx.mark_backup_committed().unwrap();
    drop(tx);

    let journal_path = root.join("transactions").join(&id).join("journal.json");
    let mut journal: JournalV2 =
        serde_json::from_str(&fs::read_to_string(&journal_path).unwrap()).unwrap();
    journal.phase = "FUTURE_PHASE".into();
    journal.checksum.clear();
    let checksum = Sha256::digest(serde_json::to_vec(&journal).unwrap());
    journal.checksum =
        checksum
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
    fs::write(
        &journal_path,
        format!("{}\n", serde_json::to_string_pretty(&journal).unwrap()),
    )
    .unwrap();

    let (_status, stdout, stderr) = run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    let record = report["journalRecords"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["installId"] == id)
        .expect("unknown-phase journal record");
    assert_eq!(record["action"], "refuse");
    assert_eq!(record["originalValid"], true);
    assert_eq!(record["recovery"], "manual");
}
