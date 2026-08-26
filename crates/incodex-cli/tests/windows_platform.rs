#![cfg(target_os = "windows")]

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
    assert!(text(&help.stdout).contains("Usage:\n  incodex"));
    assert!(help.stderr.is_empty());

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
        ("open", &["open", "--dry-run"]),
        ("install", &["install", "--dry-run"]),
        ("uninstall", &["uninstall", "--dry-run"]),
        ("status", &["status"]),
        ("doctor", &["doctor"]),
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
