use std::process::{Command, Stdio};
use std::time::Instant;

#[path = "support/readonly.rs"]
mod readonly_support;
mod support;

use readonly_support::{bin, isolated_home, run};

#[test]
fn command_help_contract_covers_every_product_command() {
    let home = isolated_home();
    let cases: &[(&str, &str)] = &[
        (
            "status",
            "\
Usage:
  incodex status [--json] [--app <path>]

Show whether Incodex is installed in Codex.

Examples:
  incodex status
  incodex status --json
",
        ),
        (
            "doctor",
            "\
Usage:
  incodex doctor [--json] [--deep] [--app <path>]

Diagnose the install, Runtime files, backup, journals, and leftover sessions.
The default checks Incodex-owned state and minimal app identity evidence.
Use --deep to inspect nested signing, entitlements, and Gatekeeper.

Flags:
  --deep            Inspect nested signing, entitlements, and Gatekeeper

Examples:
  incodex doctor
  incodex doctor --json
  incodex doctor --deep
",
        ),
        (
            "install",
            "\
Usage:
  incodex install [flags]

Patch Codex. With no flags this is the app at /Applications/ChatGPT.app.

Flags:
  --yes            Skip the confirmation prompt (required when stdin is not a terminal)
  --dry-run, -n    Print the plan and exit
  --clone          Patch a copy at ~/.incodex/scratch/ChatGPT.app
  --app <path>     Patch a specific .app

Examples:
  incodex install
  incodex install --yes
  incodex install --dry-run
  incodex install --clone
",
        ),
        (
            "uninstall",
            "\
Usage:
  incodex uninstall [flags]

Restore Codex to the snapshot taken at install. With no flags this is
/Applications/ChatGPT.app.

Flags:
  --yes            Skip the confirmation prompt (required when stdin is not a terminal)
  --dry-run, -n    Print the plan and exit
  --clone          Restore ~/.incodex/scratch/ChatGPT.app
  --app <path>     Restore a specific .app

Examples:
  incodex uninstall
  incodex uninstall --yes
  incodex uninstall --dry-run
",
        ),
        (
            "runtime",
            "\
Usage:
  incodex runtime

Write Incodex's own code to ~/.incodex/runtime/. Does not modify Codex.
Reopen Codex to load it.

Examples:
  incodex runtime
",
        ),
        (
            "recover",
            "\
Usage:
  incodex recover --transaction <id>

Roll back an install that stopped halfway. Uncommitted work is never continued.

Examples:
  incodex recover --transaction <id>
",
        ),
        (
            "open",
            "\
Usage:
  incodex open [--dry-run] [--mask] [--name <text>] [--avatar <local-file>] [--app <path>]

Open an incognito window without patching Codex. Uses an isolated CODEX_HOME
and Chromium user-data-dir. The hat-glasses control and banner still appear
in that window. Closing the window burns that session.

Profile masking is only available with --mask. Without --name, Incodex creates
a temporary name and deterministic avatar. --avatar accepts a local PNG,
JPEG, or WebP file.

Examples:
  incodex open
  incodex open --dry-run
  incodex open --mask --name \"Temporary\" --avatar ./avatar.png
",
        ),
        (
            "update",
            "\
Usage:
  inc update [--dry-run]

Update the CLI through its installation channel. Homebrew installs run
brew update and brew upgrade incodex. Script installs re-run install.sh.
Source checkouts should git pull.

Examples:
  inc update
  inc update --dry-run
",
        ),
        (
            "self-uninstall",
            "\
Usage:
  incodex self-uninstall [--restore-app] [--yes] [--dry-run]

Remove the CLI from PATH. Does not restore Codex unless --restore-app.

Examples:
  incodex self-uninstall
  incodex self-uninstall --restore-app --yes
  incodex self-uninstall --dry-run
",
        ),
    ];
    for (command, body) in cases {
        let expected = format!("{body}\n");
        for flag in ["--help", "-h"] {
            let (status, stdout, stderr) = run(&[command, flag], &home);
            assert_eq!(status, 0, "{command} {flag}");
            assert_eq!(stderr, "", "{command} {flag}");
            assert_eq!(stdout, expected, "{command} {flag}");
        }
    }
}

#[test]
fn version_wins_over_help_and_help_rejects_version_flag() {
    let home = isolated_home();
    let (status, stdout, stderr) = run(&["version", "--help"], &home);
    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    assert!(stdout.starts_with(&format!("Incodex version {}\n", env!("CARGO_PKG_VERSION"))));

    let (status, stdout, stderr) = run(&["help", "--version"], &home);
    assert_eq!(status, 1);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "  ✗ unknown flag: --version\n  incodex --help\n");
}

#[test]
fn status_missing_app_warns_and_does_not_write_incodex() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let (status, stdout, stderr) = run(&["status", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    assert_eq!(
        stdout,
        format!(
            "➤ Status\n  ! Codex app not found: {}\n  CLI Runtime  {}\n  Runtime state missing\n\n",
            app.display(),
            env!("CARGO_PKG_VERSION")
        )
    );
    assert!(!home.join(".incodex").exists());
}

#[test]
fn unknown_flags_fail_closed() {
    let home = isolated_home();
    let cases: &[(&[&str], &str)] = &[
        (&["wipe"], "  ✗ unknown command: wipe\n  incodex --help\n"),
        (
            &["status", "--please"],
            "  ✗ unknown flag: --please\n  incodex --help\n",
        ),
        (
            &["status", "--app"],
            "  ✗ --app requires a path, not another flag\n",
        ),
        (
            &["recover"],
            "  ✗ recover requires --transaction <id>\n  incodex recover --transaction <id>\n",
        ),
    ];
    for (args, stderr) in cases {
        let ran = run(args, &home);
        assert_eq!(ran.0, 1, "{args:?}");
        assert_eq!(ran.1, "", "{args:?}");
        assert_eq!(ran.2, *stderr, "{args:?}");
    }
}

#[test]
fn status_cold_start_is_recorded_against_150ms() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let app_s = app.to_str().unwrap();
    let _ = Command::new(bin())
        .args(["status", "--app", app_s])
        .env("HOME", &home)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let mut samples = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        let status = Command::new(bin())
            .args(["status", "--app", app_s])
            .env("HOME", &home)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("spawn status");
        samples.push(start.elapsed());
        assert!(status.success());
    }
    samples.sort();
    let median = samples[2];
    eprintln!(
        "incodex status median after warmup: {:?} (target 150ms); samples {:?}",
        median, samples
    );
    assert!(
        median.as_millis() <= 150,
        "median {:?} exceeds 150ms; samples {:?}",
        median,
        samples
    );
}
