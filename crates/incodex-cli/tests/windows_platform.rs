#![cfg(target_os = "windows")]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn run(args: &[&str], profile: &PathBuf) -> Output {
    Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(args)
        .env("HOME", profile)
        .env("USERPROFILE", profile)
        .env("LOCALAPPDATA", profile.join("AppData/Local"))
        .env("NO_COLOR", "1")
        .output()
        .expect("run incodex")
}

fn scratch_profile() -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "incodex-windows-platform-{}-{sequence}",
        std::process::id()
    ))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

#[test]
fn help_and_version_are_available_without_creating_state() {
    let profile = scratch_profile();

    let help = run(&["--help"], &profile);
    assert!(help.status.success(), "{}", text(&help.stderr));
    let help_text = text(&help.stdout);
    assert!(help_text.contains("Usage:\n  incodex"));
    assert!(help_text.contains("Windows"));
    assert!(help_text.contains("Microsoft Store"));
    assert!(!help_text.contains("Patch the Codex app"));
    assert!(!help_text.contains("inc is the same program"));
    assert!(help.stderr.is_empty());

    for (command, forbidden) in [("status", "--app"), ("doctor", "--deep"), ("open", "--app")] {
        let command_help = run(&[command, "--help"], &profile);
        assert!(
            command_help.status.success(),
            "{}: {}",
            command,
            text(&command_help.stderr)
        );
        let command_help_text = text(&command_help.stdout);
        assert!(command_help_text.contains("Windows"), "{command_help_text}");
        assert!(
            !command_help_text.contains(forbidden),
            "{command_help_text}"
        );
    }

    let version = run(&["--version"], &profile);
    assert!(version.status.success(), "{}", text(&version.stderr));
    let report = text(&version.stdout);
    assert!(report.starts_with("Incodex version "));
    assert!(report.contains("Windows: "));
    assert!(!report.contains("macOS: "));
    assert!(report.contains("Install: Unsupported"));
    assert!(version.stderr.is_empty());

    assert!(!profile.exists(), "read-only commands created user state");
}

#[test]
fn unsupported_product_commands_fail_closed_before_creating_state() {
    let cases: &[(&str, &[&str])] = &[
        ("runtime", &["runtime"]),
        (
            "recover",
            &[
                "recover",
                "--transaction",
                "11111111-1111-4111-8111-111111111111",
            ],
        ),
        ("update", &["update", "--dry-run"]),
        ("self-uninstall", &["self-uninstall", "--dry-run"]),
    ];

    for (command, args) in cases {
        let profile = scratch_profile();
        let output = run(args, &profile);
        assert_eq!(output.status.code(), Some(1), "{command}");
        assert!(
            output.stdout.is_empty(),
            "{command}: {}",
            text(&output.stdout)
        );
        assert!(
            text(&output.stderr).contains(&format!("{command} is not supported on Windows yet")),
            "{command}: {}",
            text(&output.stderr)
        );
        assert!(
            !profile.exists(),
            "{command} created state before its Windows implementation exists"
        );
    }
}

#[test]
fn install_and_uninstall_dry_run_discover_the_store_package_without_writing_state() {
    for (command, heading) in [("install", "Install"), ("uninstall", "Uninstall")] {
        let profile = scratch_profile();
        let output = run(&[command, "--dry-run"], &profile);
        let stdout = text(&output.stdout);
        let stderr = text(&output.stderr);

        assert!(output.status.success(), "{command}: {stderr}");
        assert!(stdout.contains(heading), "{command}: {stdout}");
        assert!(stdout.contains("OpenAI.Codex"), "{command}: {stdout}");
        assert!(stdout.contains("ChatGPT.exe"), "{command}: {stdout}");
        assert!(
            stdout.contains("Dry run. No files changed."),
            "{command}: {stdout}"
        );
        assert!(stderr.is_empty(), "{command}: {stderr}");
        assert!(
            !profile.exists(),
            "{command} dry run created product state"
        );
    }
}

#[test]
fn status_reports_current_user_package_without_creating_state() {
    let profile = scratch_profile();

    let status = run(&["status"], &profile);
    assert!(status.status.success(), "{}", text(&status.stderr));
    let report = text(&status.stdout);
    assert!(report.starts_with("➤ Status"), "{report}");
    assert!(report.ends_with("\n\n"), "{report:?}");
    assert!(!report.contains("Windows Codex"), "{report}");
    assert!(report.contains("Available"), "{report}");
    assert!(status.stderr.is_empty(), "{}", text(&status.stderr));

    let json = run(&["status", "--json"], &profile);
    assert!(json.status.success(), "{}", text(&json.stderr));
    let value: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("status emits valid JSON");
    assert_eq!(value["platform"], "windows");
    assert!(value["available"].is_boolean());
    assert!(json.stderr.is_empty(), "{}", text(&json.stderr));

    assert!(!profile.exists(), "status created user state");
}

#[test]
fn doctor_reports_package_and_session_health_without_creating_state() {
    let profile = scratch_profile();

    let doctor = run(&["doctor"], &profile);
    assert!(doctor.status.success(), "{}", text(&doctor.stderr));
    let report = text(&doctor.stdout);
    assert!(report.ends_with("\n\n"), "{report:?}");
    for expected in [
        "➤ App", "Package", "Sessions", "Active", "Orphaned", "Unknown",
    ] {
        assert!(report.contains(expected), "missing {expected}: {report}");
    }
    assert!(!report.contains("Windows Doctor"), "{report}");
    assert!(doctor.stderr.is_empty(), "{}", text(&doctor.stderr));

    let json = run(&["doctor", "--json"], &profile);
    assert!(json.status.success(), "{}", text(&json.stderr));
    let value: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("doctor emits valid JSON");
    assert_eq!(value["platform"], "windows");
    assert!(value["package"]["available"].is_boolean());
    assert!(value["sessions"]["active"].is_u64());
    assert!(value["sessions"]["orphaned"].is_u64());
    assert!(value["sessions"]["unknown"].is_u64());
    assert!(json.stderr.is_empty(), "{}", text(&json.stderr));

    assert!(!profile.exists(), "doctor created user state");
}

#[test]
fn doctor_uses_the_token_profile_instead_of_overridden_userprofile() {
    let profile = scratch_profile();
    let baseline = run(&["doctor", "--json"], &profile);
    assert!(baseline.status.success(), "{}", text(&baseline.stderr));
    let baseline: serde_json::Value =
        serde_json::from_slice(&baseline.stdout).expect("baseline doctor JSON");
    fs::create_dir_all(profile.join(".incodex/sessions"))
        .expect("create misleading environment profile");
    fs::write(profile.join(".incodex/sessions/not-a-session"), b"fixture")
        .expect("create misleading session entry");

    let doctor = run(&["doctor", "--json"], &profile);
    assert!(doctor.status.success(), "{}", text(&doctor.stderr));
    let value: serde_json::Value =
        serde_json::from_slice(&doctor.stdout).expect("doctor emits valid JSON");
    assert_eq!(value["sessions"], baseline["sessions"]);

    fs::remove_dir_all(profile).expect("remove misleading environment profile");
}

#[test]
fn open_dry_run_enters_windows_discovery_without_creating_state() {
    let profile = scratch_profile();
    let output = run(&["open", "--dry-run"], &profile);
    let stdout = text(&output.stdout);
    let stderr = text(&output.stderr);

    assert!(
        !stderr.contains("open is not supported on Windows yet"),
        "{stderr}"
    );
    if output.status.success() {
        assert!(stdout.contains("Open incognito without patching Codex"));
        assert!(stdout.contains("ChatGPT.exe"));
        assert!(stdout.contains("Dry run. No window opened."));
    } else {
        assert!(
            stderr.contains("Microsoft Store package")
                || stderr.contains("Windows package query failed"),
            "{stderr}"
        );
    }
    assert!(
        !profile.join(".incodex").exists(),
        "dry run created Incodex product state"
    );
}
