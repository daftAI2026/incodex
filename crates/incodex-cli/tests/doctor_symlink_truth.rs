use std::fs;

#[path = "support/readonly.rs"]
mod readonly_support;
mod support;

use readonly_support::{isolated_home, parse_json, run};

fn assert_unknown_finding(report: &serde_json::Value, check: &str, code: &str) {
    assert_eq!(
        report["checks"][check]["status"], "unknown",
        "{check}: {code}"
    );
    assert!(
        report["checks"][check]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == code),
        "{check} is missing {code}: {}",
        report["checks"][check]["findings"]
    );
}

#[test]
fn doctor_json_rejects_uninspected_symlink_state_without_following_it() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let root = home.join(".incodex");
    let sessions = root.join("sessions");
    fs::create_dir_all(&sessions).unwrap();

    let outside_target = home.join("outside-target");
    fs::create_dir_all(&outside_target).unwrap();
    std::os::unix::fs::symlink(&outside_target, sessions.join("target-symlink")).unwrap();

    let orphan = sessions.join("target-real/s-orphan");
    fs::create_dir_all(&orphan).unwrap();
    fs::write(
        orphan.join("owner.json"),
        serde_json::json!({"sessionId": "s-orphan", "pid": 999_999_999_i64}).to_string(),
    )
    .unwrap();
    for name in ["chromium", "incognito-chromium"] {
        let outside = home.join(format!("outside-{name}"));
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(outside, orphan.join(name)).unwrap();
    }

    for name in ["incognito-home", "incognito-chromium"] {
        let outside = home.join(format!("outside-legacy-{name}"));
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(outside, root.join(name)).unwrap();
    }

    let outside_targets = home.join("outside-targets");
    let foreign_target = outside_targets.join("foreign");
    fs::create_dir_all(&foreign_target).unwrap();
    fs::write(
        foreign_target.join("incognito.lock"),
        serde_json::json!({
            "pid": 999_999_999_i64,
            "processStartIdentity": "never-started",
            "execIdentity": "ChatGPT",
            "token": "0123456789abcdef0123456789abcdef"
        })
        .to_string(),
    )
    .unwrap();
    std::os::unix::fs::symlink(&outside_targets, root.join("targets")).unwrap();

    std::os::unix::fs::symlink(home.join("missing-transactions"), root.join("transactions"))
        .unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_eq!(report["stalePid"], false);
    assert_unknown_finding(&report, "processIdentity", "owner.targets-symlink");
    assert_unknown_finding(&report, "orphanSessions", "session.target-symlink");
    assert_unknown_finding(&report, "orphanSessions", "chromium.session-symlink");
    assert_unknown_finding(&report, "chromiumResidue", "chromium.session-symlink");
    assert_unknown_finding(&report, "chromiumResidue", "chromium.legacy-symlink");
    assert_unknown_finding(&report, "journals", "journal.root-symlink");
}

#[test]
fn doctor_json_rejects_dangling_sessions_root_symlink() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let root = home.join(".incodex");
    fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(home.join("missing-sessions"), root.join("sessions")).unwrap();
    std::os::unix::fs::symlink(home.join("missing-transactions"), root.join("transactions"))
        .unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_unknown_finding(&report, "orphanSessions", "session.root-symlink");
    assert_unknown_finding(&report, "chromiumResidue", "session.root-symlink");
    assert_unknown_finding(&report, "journals", "journal.root-symlink");
}

#[test]
fn doctor_json_rejects_a_symlinked_journal_file_without_reading_target() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let transactions = home.join(".incodex/transactions");
    let outside = home.join("outside-journal.json");
    fs::create_dir_all(&transactions).unwrap();
    fs::write(&outside, b"{}").unwrap();
    let journal = transactions.join("journal.json");
    std::os::unix::fs::symlink(&outside, &journal).unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_unknown_finding(&report, "journals", "journal.file-symlink");
    assert!(report["journalRecords"]
        .as_array()
        .unwrap()
        .iter()
        .any(|record| {
            record["kind"] == "symlink" && record["path"] == journal.display().to_string()
        }));
    assert_eq!(fs::read_to_string(outside).unwrap(), "{}");
}

#[test]
fn doctor_json_rejects_symlinked_target_and_native_journal_paths() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let root = home.join(".incodex");

    let targets = root.join("targets");
    let outside_target = home.join("outside-target");
    fs::create_dir_all(&outside_target).unwrap();
    fs::write(
        outside_target.join("incognito.lock"),
        serde_json::json!({
            "pid": 999_999_999_i64,
            "processStartIdentity": "never-started",
            "execIdentity": "ChatGPT",
            "token": "0123456789abcdef0123456789abcdef"
        })
        .to_string(),
    )
    .unwrap();
    fs::create_dir_all(&targets).unwrap();
    std::os::unix::fs::symlink(&outside_target, targets.join("target-symlink")).unwrap();

    let transactions = root.join("transactions");
    let native_id = "01234567-89ab-4cde-8123-456789abcdef";
    let native_transaction = transactions.join(native_id);
    let outside_journal = home.join("outside-native-journal.json");
    fs::create_dir_all(&native_transaction).unwrap();
    fs::write(&outside_journal, b"{}").unwrap();
    std::os::unix::fs::symlink(&outside_journal, native_transaction.join("journal.json")).unwrap();

    let direct_id = "fedcba98-7654-4321-8fed-cba987654321";
    let outside_transaction = home.join("outside-transaction");
    fs::create_dir_all(&outside_transaction).unwrap();
    fs::write(outside_transaction.join("journal.json"), b"{}").unwrap();
    std::os::unix::fs::symlink(&outside_transaction, transactions.join(direct_id)).unwrap();

    let (_status, stdout, stderr) =
        run(&["doctor", "--json", "--app", app.to_str().unwrap()], &home);
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_unknown_finding(&report, "processIdentity", "owner.target-symlink");
    assert_unknown_finding(&report, "journals", "journal.file-symlink");
    assert_unknown_finding(&report, "journals", "journal.transaction-symlink");
    assert!(fs::symlink_metadata(targets.join("target-symlink"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(
        fs::symlink_metadata(native_transaction.join("journal.json"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(fs::symlink_metadata(transactions.join(direct_id))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read_to_string(outside_journal).unwrap(), "{}");
    assert_eq!(
        fs::read_to_string(outside_transaction.join("journal.json")).unwrap(),
        "{}"
    );
}
