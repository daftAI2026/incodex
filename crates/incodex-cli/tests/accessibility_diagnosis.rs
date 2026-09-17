#![cfg(target_os = "macos")]

use incodex_cli::diagnose::{diagnose_with_root_mode, DiagnosisMode};

#[test]
fn doctor_does_not_substitute_the_terminal_permission_for_an_absent_app() {
    let root = std::env::temp_dir().join(format!(
        "incodex-accessibility-absent-{}",
        std::process::id()
    ));
    let app = root.join("Absent.app");
    assert!(!app.exists());
    let report = diagnose_with_root_mode(&app, &root, DiagnosisMode::Doctor);
    let value = serde_json::to_value(report).unwrap();
    assert_eq!(value["accessibility"]["status"], "unknown");
    assert_eq!(value["accessibility"]["pid"], serde_json::Value::Null);
    assert!(!root.exists(), "diagnosis must remain read-only");
}

#[test]
fn status_does_not_silently_request_accessibility_permission() {
    let root = std::env::temp_dir().join(format!(
        "incodex-accessibility-status-{}",
        std::process::id()
    ));
    let report = diagnose_with_root_mode(&root.join("Absent.app"), &root, DiagnosisMode::Status);
    let value = serde_json::to_value(report).unwrap();
    assert_eq!(value["accessibility"]["status"], "notRequested");
    assert!(!root.exists());
}
