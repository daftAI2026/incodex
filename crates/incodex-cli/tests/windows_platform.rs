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

fn store_package_unavailable(message: &str) -> bool {
    message.contains("official Codex Microsoft Store package is not installed")
        || message.contains("Windows package query failed")
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
    assert!(help_text.contains("Unsupported source preview"));
    assert!(help_text.contains("Enable the hat-glasses control"));
    assert!(help_text.contains("Remove the Windows Runtime integration"));
    assert!(!help_text.contains("install      Not available"));
    assert!(!help_text.contains("runtime      Not available"));
    assert!(!help_text.contains("update       Not available"));
    assert!(!help_text.contains("Patch the Codex app"));
    assert!(!help_text.contains("inc is the same program"));
    assert!(help.stderr.is_empty());

    for (command, forbidden) in [
        ("status", "--app"),
        ("doctor", "--deep"),
        ("open", "--app"),
        ("install", "Patch Codex"),
        ("uninstall", "Restore Codex"),
    ] {
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
            command_help_text.contains("Unsupported source preview"),
            "{command_help_text}"
        );
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
    assert!(report.contains("Install: Source"));
    assert!(version.stderr.is_empty());

    assert!(!profile.exists(), "read-only commands created user state");
}

#[test]
fn unsupported_product_commands_fail_closed_before_creating_state() {
    let cases: &[(&str, &[&str])] = &[
        (
            "recover",
            &[
                "recover",
                "--transaction",
                "11111111-1111-4111-8111-111111111111",
            ],
        ),
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
fn runtime_and_managed_update_have_windows_dry_run_contracts() {
    let profile = scratch_profile();
    let runtime = run(&["runtime", "--dry-run"], &profile);
    assert!(runtime.status.success(), "{}", text(&runtime.stderr));
    assert!(
        text(&runtime.stdout).contains("would publish the embedded Runtime"),
        "{}",
        text(&runtime.stdout)
    );

    let package_root = profile.join("managed/packages/standalone");
    let update = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["update", "--dry-run"])
        .env("INCODEX_MANAGED_BY_STANDALONE", "1")
        .env("INCODEX_MANAGED_PACKAGE_ROOT", &package_root)
        .env("NO_COLOR", "1")
        .output()
        .expect("run managed update dry-run");
    assert!(update.status.success(), "{}", text(&update.stderr));
    let stdout = text(&update.stdout);
    assert!(stdout.contains("Updating Incodex via `"), "{stdout}");
    assert!(stdout.contains("install.ps1"), "{stdout}");
    assert!(
        stdout.contains("would publish Runtime with the installed CLI"),
        "{stdout}"
    );
    assert!(stdout.contains("no changes made."), "{stdout}");
    assert!(!profile.exists(), "dry-run created product state");
}

#[test]
fn update_from_an_unmanaged_windows_binary_fails_with_install_guidance() {
    let profile = scratch_profile();
    let output = run(&["update", "--dry-run"], &profile);
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("not a managed Windows installation"),
        "{stderr}"
    );
    assert!(stderr.contains("install.ps1"), "{stderr}");
    assert!(!profile.exists(), "unmanaged update created user state");
}

#[test]
fn install_and_uninstall_dry_run_require_discovery_without_writing_state() {
    for (command, heading) in [("install", "Install"), ("uninstall", "Uninstall")] {
        let profile = scratch_profile();
        let output = run(&[command, "--dry-run"], &profile);
        let stdout = text(&output.stdout);
        let stderr = text(&output.stderr);

        if output.status.success() {
            assert!(stdout.contains(heading), "{command}: {stdout}");
            if command == "install" {
                assert!(stdout.contains("OpenAI.Codex"), "{command}: {stdout}");
                assert!(stdout.contains("ChatGPT.exe"), "{command}: {stdout}");
            } else {
                assert!(
                    stdout.contains("OpenAI.Codex") || stdout.contains("Not discovered"),
                    "{command}: {stdout}"
                );
                assert!(
                    stdout.contains("ChatGPT.exe") || stdout.contains("Unavailable"),
                    "{command}: {stdout}"
                );
            }
            assert!(
                stdout.contains("Dry run. No files changed."),
                "{command}: {stdout}"
            );
            assert!(stderr.is_empty(), "{command}: {stderr}");
        } else {
            assert_eq!(command, "install", "{command}: {stderr}");
            assert!(store_package_unavailable(&stderr), "{command}: {stderr}");
            assert!(stdout.is_empty(), "{command}: {stdout}");
        }
        assert!(!profile.exists(), "{command} dry run created product state");
    }
}

#[test]
fn windows_install_commands_reject_macos_target_selectors_before_discovery() {
    for args in [
        &["install", "--clone", "--dry-run"][..],
        &["install", "--app", r"C:\Fake\ChatGPT.app", "--dry-run"][..],
        &["uninstall", "--clone", "--dry-run"][..],
        &["uninstall", "--app", r"C:\Fake\ChatGPT.app", "--dry-run"][..],
    ] {
        let profile = scratch_profile();
        let output = run(args, &profile);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(
            text(&output.stderr).contains("not supported on Windows"),
            "{args:?}: {}",
            text(&output.stderr)
        );
        assert!(!profile.exists(), "{args:?} created state");
    }
}

#[test]
fn uninstall_reads_durable_state_before_optional_store_discovery() {
    let source = include_str!("../src/windows_install.rs");
    let start = source
        .find("pub fn run_uninstall")
        .expect("uninstall entry");
    let end = source[start..]
        .find("pub enum WindowsUninstallOutcome")
        .map(|offset| start + offset)
        .expect("uninstall entry end");
    let uninstall = &source[start..end];
    let state_read = uninstall
        .find("read_windows_install_state")
        .expect("durable state read");
    let discovery = uninstall
        .find("discover_codex_package")
        .expect("optional Store discovery");
    assert!(state_read < discovery);
    assert!(!uninstall.contains("discover_codex_package()?"));
}

#[test]
fn windows_open_holds_the_install_state_gate_across_selection_and_activation() {
    let source = include_str!("../src/windows_open.rs");
    let launch = source
        .split("fn launch_windows_open(")
        .nth(1)
        .expect("Windows open launch boundary");
    let launch = launch
        .split("fn run_windows_open_lifecycle")
        .next()
        .expect("bounded launch source");
    let acquired = launch
        .find("acquire_windows_install_state")
        .expect("launch acquires the install-state gate");
    let read = launch
        .find("read_windows_install_state")
        .expect("launch reads install state under the gate");
    let activated = launch
        .find("activate_packaged")
        .expect("launch activates while the gate is held");

    assert!(acquired < read);
    assert!(read < activated);
}

#[test]
fn windows_open_dry_run_resolves_and_reports_profile_mask() {
    let profile = scratch_profile();
    let baseline = run(&["open", "--dry-run"], &profile);
    if !baseline.status.success() {
        let stderr = text(&baseline.stderr);
        assert!(
            stderr.contains("Microsoft Store package")
                || stderr.contains("Windows package query failed"),
            "unexpected package discovery failure: {stderr}"
        );
        return;
    }

    fs::create_dir_all(&profile).expect("create avatar fixture directory");
    let avatar = profile.join("avatar.png");
    fs::write(&avatar, b"\x89PNG\r\n\x1a\nfixture").expect("write avatar fixture");
    let avatar = avatar.to_string_lossy().into_owned();
    let missing_avatar = profile.join("missing-avatar.png");
    let missing_avatar = missing_avatar.to_string_lossy().into_owned();

    let invalid = run(
        &["open", "--dry-run", "--mask", "--avatar", &missing_avatar],
        &profile,
    );
    assert_eq!(invalid.status.code(), Some(1), "{}", text(&invalid.stdout));
    assert!(
        text(&invalid.stderr).contains("cannot open avatar file"),
        "{}",
        text(&invalid.stderr)
    );

    let valid = run(
        &[
            "open",
            "--dry-run",
            "--mask",
            "--name",
            "Temporary",
            "--avatar",
            &avatar,
        ],
        &profile,
    );
    assert!(valid.status.success(), "{}", text(&valid.stderr));
    let stdout = text(&valid.stdout);
    assert!(stdout.contains("Profile"), "{stdout}");
    assert!(stdout.contains("Temporary"), "{stdout}");
    assert!(!profile.join(".incodex").exists(), "dry-run created state");
}

#[test]
fn mutating_install_never_reaches_mutation_without_confirmation() {
    let profile = scratch_profile();
    let output = run(&["install"], &profile);
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("non-interactive install requires --yes")
            || store_package_unavailable(&stderr),
        "{stderr}"
    );
    assert!(!profile.exists(), "unconfirmed install created state");
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
    assert!(report.contains("Installed"), "{report}");
    assert!(status.stderr.is_empty(), "{}", text(&status.stderr));

    let json = run(&["status", "--json"], &profile);
    assert!(json.status.success(), "{}", text(&json.stderr));
    let value: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("status emits valid JSON");
    assert_eq!(value["platform"], "windows");
    assert!(value["available"].is_boolean());
    assert!(value["integration"]["installed"].is_boolean());
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
    assert!(report.contains("Integration"), "{report}");
    assert!(report.contains("Installed"), "{report}");
    for expected in [
        "➤ App",
        "Available",
        "Sessions",
        "Active",
        "Orphaned",
        "Unknown",
    ] {
        assert!(report.contains(expected), "missing {expected}: {report}");
    }
    if report.contains("official Codex Microsoft Store package is not installed") {
        assert!(!report.contains("Package"), "{report}");
    } else {
        assert!(report.contains("Package"), "{report}");
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
    assert!(value["integration"]["installed"].is_boolean());
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
