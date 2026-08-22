use std::fs;
use std::os::unix::fs::MetadataExt;

use incodex_asar::{pack_dir, patch_asar, MARKER_KEY};
use incodex_macos::ditto;
use incodex_transaction::Engine;
use sha2::{Digest, Sha256};

#[path = "support/readonly.rs"]
mod readonly_support;
mod support;
#[path = "doctor/transaction_evidence.rs"]
mod transaction_evidence;

use readonly_support::{
    isolated_home, parse_json, run, run_with_stdout_redirected, top_level_json_keys, DIAGNOSIS_KEYS,
};

#[test]
fn doctor_missing_app_prints_labeled_sections() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let (status, stdout, stderr) = run(&["doctor", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    assert_eq!(
        stdout,
        format!(
            "\
➤ App
  Path         {app}
  Exists       no
  Installed    no
  Bundle       unknown
  Version      unknown
  Arch         unknown

➤ Runtime
  Version      unknown
  External     missing
  External check checked
  ! missing current.json
  Loader       unknown
  Main         unknown

➤ Signing
  Verify       failed
  Nested       unknown

➤ Backup
  State        none
  Proof        checked

➤ Sessions
  Orphans      0 (checked)
  Chromium     0 (checked)
  Stale pid    no (checked)
  Journals     0 (checked)

➤ Findings
  ! signing.not-checked: the application does not exist, so nested signing was not inspected

",
            app = app.display()
        )
    );
}

#[test]
fn status_and_doctor_do_not_animate_when_stdout_is_redirected() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    for command in ["status", "doctor"] {
        let (status, stdout, stderr) =
            run_with_stdout_redirected(&[command, "--app", app.to_str().unwrap()], &home);
        assert_eq!(status, 0, "{command}: {stderr:?}");
        assert!(stdout.contains(if command == "status" {
            "➤ Status"
        } else {
            "➤ App"
        }));
        assert_eq!(stderr, "", "{command} leaked TTY progress: {stderr:?}");
    }
}

#[test]
fn status_json_and_doctor_json_share_diagnosis_object() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let app_s = app.to_str().unwrap();
    let status = run(&["status", "--json", "--app", app_s], &home);
    let doctor = run(&["doctor", "--json", "--app", app_s], &home);
    assert_eq!(status.0, 0);
    assert_eq!(doctor.0, 0);
    assert_eq!(status.2, "");
    assert_eq!(doctor.2, "");
    assert_eq!(status.1, doctor.1);

    let rec = parse_json(&status.1);
    let keys = top_level_json_keys(&status.1);
    assert_eq!(keys, DIAGNOSIS_KEYS);
    assert_eq!(rec["target"], app_s);
    assert!(rec["targetId"].as_str().unwrap().starts_with("app-"));
    assert_eq!(rec["targetId"].as_str().unwrap().len(), 16);
    assert_eq!(rec["exists"], false);
    assert_eq!(rec["patched"], false);
    assert!(rec["bundleId"].is_null());
    assert_eq!(rec["originalMain"], "");
    assert_eq!(rec["codesignOk"], false);
    assert!(rec["backup"].is_null());
    assert_eq!(rec["stalePid"], false);
    assert_eq!(rec["orphanSessions"], serde_json::json!([]));
    assert_eq!(rec["asarLoaderOnly"], serde_json::Value::Null);
    assert!(rec["signing"].is_null());
    assert!(rec["spctl"].is_null());
    assert_eq!(rec["interruptedTransactions"], serde_json::json!([]));
    let runtime = &rec["externalRuntime"];
    assert_eq!(runtime["present"], false);
    assert_eq!(runtime["ok"], false);
    assert!(runtime["version"].is_null());
    assert_eq!(runtime["error"], "missing current.json");
}

#[test]
fn doctor_accepts_a_runtime_publisher_pointer_with_optional_manifest_fields() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let published = incodex_runtime_bundle::publish(&home.join(".incodex")).unwrap();

    let (status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    let runtime = &parse_json(&stdout)["externalRuntime"];
    assert_eq!(runtime["present"], true);
    assert_eq!(runtime["ok"], true);
    assert_eq!(runtime["version"], published.version);
    assert_eq!(runtime["release"], published.release);
}

#[test]
fn doctor_rejects_runtime_manifest_missing_required_artifacts() {
    let home = isolated_home();
    let release = home.join(".incodex/runtime/releases/0.2.0");
    fs::create_dir_all(&release).expect("runtime release");

    let body = b"valid runtime artifact\n";
    fs::write(release.join("incodex-main.cjs"), body).expect("runtime artifact");
    let hash = Sha256::digest(body)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    fs::write(
        home.join(".incodex/runtime/current.json"),
        serde_json::json!({
            "schemaVersion": 1,
            "version": "0.2.0",
            "release": "releases/0.2.0",
            "files": { "incodex-main.cjs": hash },
        })
        .to_string(),
    )
    .expect("runtime manifest");

    let app = home.join("Missing.app");
    for command in ["status", "doctor"] {
        let (status, stdout, stderr) =
            run(&[command, "--json", "--app", app.to_str().unwrap()], &home);
        assert_eq!(status, 0, "{command}");
        assert_eq!(stderr, "", "{command}");
        let runtime = &parse_json(&stdout)["externalRuntime"];
        assert_eq!(runtime["present"], true, "{command}");
        assert_eq!(runtime["ok"], false, "{command}");
        assert!(
            runtime["error"]
                .as_str()
                .expect("runtime error")
                .contains("incodex-preload.cjs"),
            "{command}"
        );
    }
}

#[test]
fn doctor_json_marks_legacy_flat_interruption_as_manual_only() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let tx_dir = home.join(".incodex").join("transactions");
    fs::create_dir_all(&tx_dir).unwrap();
    fs::write(
        tx_dir.join("tx-contract.json"),
        serde_json::json!({
            "schemaVersion": 1,
            "installId": "tx-contract",
            "targetRealPath": app.to_str().unwrap(),
            "stagedApp": home.join("staged").to_str().unwrap(),
            "originalSnapshot": home.join("original").to_str().unwrap(),
            "phase": "PATCHED",
            "updatedAt": "2026-01-01T00:00:00.000Z"
        })
        .to_string()
            + "\n",
    )
    .unwrap();
    let (status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    let rec = parse_json(&stdout);
    let txs = rec["interruptedTransactions"].as_array().unwrap();
    assert_eq!(txs.len(), 1);
    assert_eq!(txs[0]["installId"], "tx-contract");
    assert_eq!(txs[0]["phase"], "PATCHED");
    assert_eq!(txs[0]["action"], "manual");
    let record = rec["journalRecords"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["installId"] == "tx-contract")
        .expect("legacy flat journal record");
    assert_eq!(record["action"], "manual");
    assert_eq!(record["recovery"], "manual");
}

#[test]
fn doctor_json_exposes_explicit_check_truth_and_unknown_signing() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_eq!(report["checks"]["processIdentity"]["status"], "checked");
    assert_eq!(report["checks"]["orphanSessions"]["status"], "checked");
    assert_eq!(report["checks"]["runtime"]["status"], "checked");
    assert_eq!(report["checks"]["signing"]["status"], "unknown");
    assert!(report["checks"]["signing"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "signing.not-checked"));
    assert!(report["findings"].is_array());
}

#[test]
fn doctor_json_classifies_owner_orphans_and_runtime_residue() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let root = home.join(".incodex");
    let target_id = "target-contract";
    let state_root = root.join("targets").join(target_id);
    fs::create_dir_all(&state_root).unwrap();
    fs::write(
        state_root.join("incognito.lock"),
        serde_json::json!({
            "pid": 999_999_999_i64,
            "processStartIdentity": "never-started",
            "execIdentity": "ChatGPT",
            "token": "0123456789abcdef0123456789abcdef"
        })
        .to_string(),
    )
    .unwrap();

    let session_root = root
        .join("sessions")
        .join(target_id)
        .join("s-orphan-contract");
    fs::create_dir_all(session_root.join("chromium")).unwrap();
    let metadata = fs::symlink_metadata(&session_root).unwrap();
    fs::write(
        session_root.join("owner.json"),
        serde_json::json!({
            "sessionId": "s-orphan-contract",
            "pid": 999_999_999_i64,
            "ino": metadata.ino(),
            "dev": metadata.dev()
        })
        .to_string(),
    )
    .unwrap();

    let release = root.join("runtime/releases/0.3.1");
    fs::create_dir_all(&release).unwrap();
    std::os::unix::fs::symlink(
        home.join("outside-runtime"),
        release.join("incodex-main.cjs"),
    )
    .unwrap();
    let mut files = serde_json::Map::new();
    for name in incodex_runtime_bundle::required_runtime_files() {
        let hash = if name == "incodex-main.cjs" {
            "00".repeat(32)
        } else {
            let body = b"runtime artifact";
            fs::write(release.join(name), body).unwrap();
            Sha256::digest(body)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        files.insert(name.to_string(), serde_json::Value::String(hash));
    }
    fs::write(
        root.join("runtime/current.json"),
        serde_json::json!({
            "schemaVersion": 1,
            "version": "0.3.1",
            "release": "releases/0.3.1",
            "files": files
        })
        .to_string(),
    )
    .unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_eq!(report["stalePid"], true);
    assert_eq!(report["checks"]["processIdentity"]["status"], "checked");
    assert_eq!(report["checks"]["orphanSessions"]["status"], "checked");
    assert!(report["orphanSessions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|path| path.as_str().unwrap().contains("s-orphan-contract")));
    assert_eq!(report["checks"]["runtime"]["status"], "checked");
    assert!(report["checks"]["runtime"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "runtime.symlink"));
}

#[test]
fn doctor_json_marks_a_symlinked_session_root_unknown_without_following_it() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let target = home.join(".incodex/sessions/target-contract");
    let outside = home.join("outside-session");
    let session = target.join("s-symlink-contract");
    fs::create_dir_all(&target).unwrap();
    fs::create_dir_all(outside.join("chromium")).unwrap();
    std::os::unix::fs::symlink(&outside, &session).unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_eq!(report["orphanSessions"], serde_json::json!([]));
    assert_eq!(report["leftoverChromium"], serde_json::json!([]));
    assert_eq!(report["checks"]["orphanSessions"]["status"], "unknown");
    assert_eq!(report["checks"]["chromiumResidue"]["status"], "unknown");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "session.symlink"));
    assert!(fs::symlink_metadata(&session)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(outside.join("chromium").is_dir());
}

#[test]
fn doctor_json_marks_a_flat_symlinked_session_root_unknown_without_following_it() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let sessions = home.join(".incodex/sessions");
    let outside = home.join("outside-flat-session");
    let session = sessions.join("s-old");
    fs::create_dir_all(&sessions).unwrap();
    fs::create_dir_all(outside.join("chromium")).unwrap();
    std::os::unix::fs::symlink(&outside, &session).unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_eq!(report["orphanSessions"], serde_json::json!([]));
    assert_eq!(report["leftoverChromium"], serde_json::json!([]));
    assert_eq!(report["checks"]["orphanSessions"]["status"], "unknown");
    assert_eq!(report["checks"]["chromiumResidue"]["status"], "unknown");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "session.symlink"));
    assert!(fs::symlink_metadata(&session)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(outside.join("chromium").is_dir());
}

#[test]
fn doctor_json_reports_a_symlinked_runtime_root_as_checked_finding() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let runtime_parent = home.join(".incodex");
    let outside = home.join("outside-runtime");
    let runtime_root = runtime_parent.join("runtime");
    fs::create_dir_all(&runtime_parent).unwrap();
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, &runtime_root).unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_eq!(report["externalRuntime"]["present"], true);
    assert_eq!(report["externalRuntime"]["ok"], false);
    assert_eq!(report["checks"]["runtime"]["status"], "checked");
    assert!(report["checks"]["runtime"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "runtime.symlink"));
}

#[test]
fn doctor_json_scans_legacy_chromium_residue_without_modern_sessions() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let root = home.join(".incodex");
    fs::create_dir_all(root.join("incognito-home")).unwrap();
    fs::create_dir_all(root.join("incognito-chromium")).unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    let residue = report["leftoverChromium"].as_array().unwrap();
    assert_eq!(residue.len(), 2);
    assert!(residue.iter().any(|path| {
        path.as_str()
            .is_some_and(|path| path.ends_with(".incodex/incognito-home"))
    }));
    assert!(residue.iter().any(|path| {
        path.as_str()
            .is_some_and(|path| path.ends_with(".incodex/incognito-chromium"))
    }));
    assert_eq!(report["checks"]["chromiumResidue"]["status"], "checked");
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|finding| finding["code"] == "chromium.residue")
            .count()
            >= 2
    );
}

#[test]
fn doctor_json_keeps_malformed_legacy_and_stale_committed_journals_visible() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let tx_dir = home.join(".incodex").join("transactions");
    fs::create_dir_all(&tx_dir).unwrap();
    fs::write(tx_dir.join("malformed.json"), b"{not-json\n").unwrap();
    fs::write(
        tx_dir.join("legacy.json"),
        serde_json::json!({ "schemaVersion": 99, "installId": "legacy" }).to_string(),
    )
    .unwrap();
    fs::write(
        tx_dir.join("committed.json"),
        serde_json::json!({
            "schemaVersion": 1,
            "installId": "committed",
            "targetRealPath": app.to_str().unwrap(),
            "stagedApp": home.join("staged").to_str().unwrap(),
            "originalSnapshot": home.join("original").to_str().unwrap(),
            "phase": "COMMITTED",
            "updatedAt": "2026-01-01T00:00:00.000Z"
        })
        .to_string(),
    )
    .unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    let records = report["journalRecords"].as_array().unwrap();
    assert!(records.iter().any(|record| {
        record["kind"] == "malformed" && record["path"].as_str().unwrap().contains("malformed.json")
    }));
    assert!(records.iter().any(|record| {
        record["kind"] == "unrecognizedLegacy"
            && record["path"].as_str().unwrap().contains("legacy.json")
    }));
    assert!(records.iter().any(|record| {
        record["kind"] == "staleCommitted" && record["installId"] == "committed"
    }));
    assert_eq!(report["checks"]["journals"]["status"], "checked");
    assert!(report["checks"]["journals"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "journal.malformed"));
}

#[test]
fn doctor_json_refuses_clean_backup_for_a_patched_marker_without_native_backup() {
    let home = isolated_home();
    let app = home.join("ChatGPT.app");
    let source = home.join("asar-source");
    let asar = app.join("Contents/Resources/app.asar");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(asar.parent().unwrap()).unwrap();
    fs::write(source.join("index.js"), b"official\n").unwrap();
    let install_id = "00000000-0000-4000-8000-000000000001";
    let mut package = serde_json::json!({ "main": "index.js" });
    package[MARKER_KEY] = serde_json::json!({
        "originalMain": "index.js",
        "installId": install_id,
    });
    fs::write(
        source.join("package.json"),
        format!("{}\n", serde_json::to_string(&package).unwrap()),
    )
    .unwrap();
    pack_dir(&source, &asar).unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_eq!(report["patched"], true);
    assert_eq!(report["backup"]["status"], "unknown");
    assert_eq!(report["checks"]["backup"]["status"], "unknown");
    assert!(report["checks"]["backup"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "backup.unverified"));
    assert_ne!(report["backup"]["complete"], true);
}

#[test]
fn doctor_json_does_not_call_the_live_committed_journal_stale() {
    let home = isolated_home();
    let root = home.join(".incodex");
    let app = home.join("ChatGPT.app");
    let source = home.join("asar-source");
    let candidate = home.join("candidate.app");
    let asar = app.join("Contents/Resources/app.asar");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(asar.parent().unwrap()).unwrap();
    fs::write(source.join("index.js"), b"official\n").unwrap();
    fs::write(source.join("package.json"), b"{\"main\":\"index.js\"}\n").unwrap();
    pack_dir(&source, &asar).unwrap();

    let mut transaction = Engine::begin(&root, &app, "test").unwrap();
    let install_id = transaction.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&install_id)
        .join("original/ChatGPT.app");
    fs::create_dir_all(original.parent().unwrap()).unwrap();
    ditto(&app, &original).unwrap();
    transaction.mark_backup_committed().unwrap();
    ditto(&app, &candidate).unwrap();
    patch_asar(
        &candidate.join("Contents/Resources/app.asar"),
        "module.exports = {};\n",
        Some(&install_id),
    )
    .unwrap();
    transaction.place_staging(&candidate).unwrap();
    transaction.swap().unwrap();
    transaction.commit().unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    let records = report["journalRecords"].as_array().unwrap();
    assert!(records.iter().any(|record| {
        record["kind"] == "currentCommitted" && record["installId"] == install_id
    }));
    assert!(!records
        .iter()
        .any(|record| record["kind"] == "staleCommitted"));
}

#[test]
fn doctor_json_does_not_call_a_committed_journal_with_a_missing_backup_clean() {
    let home = isolated_home();
    let root = home.join(".incodex");
    let app = home.join("ChatGPT.app");
    let source = home.join("asar-source");
    let candidate = home.join("candidate.app");
    let asar = app.join("Contents/Resources/app.asar");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(asar.parent().unwrap()).unwrap();
    fs::write(source.join("index.js"), b"official\n").unwrap();
    fs::write(source.join("package.json"), b"{\"main\":\"index.js\"}\n").unwrap();
    pack_dir(&source, &asar).unwrap();

    let mut transaction = Engine::begin(&root, &app, "test").unwrap();
    let install_id = transaction.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&install_id)
        .join("original/ChatGPT.app");
    fs::create_dir_all(original.parent().unwrap()).unwrap();
    ditto(&app, &original).unwrap();
    transaction.mark_backup_committed().unwrap();
    ditto(&app, &candidate).unwrap();
    patch_asar(
        &candidate.join("Contents/Resources/app.asar"),
        "module.exports = {};\n",
        Some(&install_id),
    )
    .unwrap();
    transaction.place_staging(&candidate).unwrap();
    transaction.swap().unwrap();
    transaction.commit().unwrap();
    fs::remove_dir_all(&original).unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_eq!(report["backup"]["status"], "unknown");
    assert_eq!(report["checks"]["backup"]["status"], "unknown");
    assert!(report["checks"]["backup"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "backup.missing"));
    assert_ne!(report["backup"]["complete"], true);
}

#[test]
fn doctor_json_rejects_a_present_but_truncated_committed_backup() {
    let home = isolated_home();
    let root = home.join(".incodex");
    let app = home.join("ChatGPT.app");
    let source = home.join("asar-source");
    let candidate = home.join("candidate.app");
    let asar = app.join("Contents/Resources/app.asar");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(asar.parent().unwrap()).unwrap();
    fs::write(source.join("index.js"), b"official\n").unwrap();
    fs::write(source.join("package.json"), b"{\"main\":\"index.js\"}\n").unwrap();
    pack_dir(&source, &asar).unwrap();

    let mut transaction = Engine::begin(&root, &app, "test").unwrap();
    let install_id = transaction.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&install_id)
        .join("original/ChatGPT.app");
    fs::create_dir_all(original.parent().unwrap()).unwrap();
    ditto(&app, &original).unwrap();
    transaction.mark_backup_committed().unwrap();
    ditto(&app, &candidate).unwrap();
    patch_asar(
        &candidate.join("Contents/Resources/app.asar"),
        "module.exports = {};\n",
        Some(&install_id),
    )
    .unwrap();
    transaction.place_staging(&candidate).unwrap();
    transaction.swap().unwrap();
    transaction.commit().unwrap();
    fs::write(
        original.join("Contents/Resources/app.asar"),
        b"truncated backup",
    )
    .unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_eq!(report["backup"]["status"], "unknown");
    assert_eq!(report["checks"]["backup"]["status"], "unknown");
    assert!(report["checks"]["backup"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "backup.digest-mismatch"));
    assert_ne!(report["backup"]["complete"], true);
}

#[test]
fn doctor_json_marks_unverifiable_owner_and_session_records_unknown() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let root = home.join(".incodex");
    let target_id = "invalid-contract";
    let state_root = root.join("targets").join(target_id);
    fs::create_dir_all(&state_root).unwrap();
    fs::write(state_root.join("incognito.lock"), b"{}\n").unwrap();
    let session = root
        .join("sessions")
        .join(target_id)
        .join("s-invalid-contract");
    fs::create_dir_all(&session).unwrap();
    fs::write(session.join("owner.json"), b"{}\n").unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_eq!(report["stalePid"], false);
    assert_eq!(report["orphanSessions"], serde_json::json!([]));
    assert_eq!(report["checks"]["processIdentity"]["status"], "unknown");
    assert_eq!(report["checks"]["orphanSessions"]["status"], "unknown");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| { finding["code"] == "owner.invalid" }));
}

#[test]
fn doctor_json_marks_a_live_session_without_process_identity_unknown() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let session = home.join(".incodex/sessions/target-contract/s-live-without-identity");
    fs::create_dir_all(session.join("chromium")).unwrap();
    fs::write(
        session.join("owner.json"),
        serde_json::json!({
            "sessionId": "s-live-without-identity",
            "pid": std::process::id(),
        })
        .to_string(),
    )
    .unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_eq!(report["checks"]["orphanSessions"]["status"], "unknown");
    assert_eq!(report["checks"]["chromiumResidue"]["status"], "unknown");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "session.identity-missing"));
}
