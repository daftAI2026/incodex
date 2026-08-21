use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "support/readonly.rs"]
mod readonly_support;
mod support;

use readonly_support::{isolated_home, parse_json, run};

struct Fixture {
    root: PathBuf,
    app: PathBuf,
    log: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "incodex-doctor-depth-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let app = root.join("ChatGPT.app");
        let fake_bin = root.join("bin");
        let log = root.join("commands.log");
        fs::create_dir_all(app.join("Contents/MacOS")).expect("app");
        fs::create_dir_all(&fake_bin).expect("fake bin");
        fs::write(app.join("Contents/MacOS/ChatGPT"), "binary\n").expect("binary");
        fs::write(
            fake_bin.join("codesign"),
            r##"#!/bin/sh
printf 'codesign %s\n' "$*" >> "$INCODEX_DOCTOR_COMMAND_LOG"
if [ "$1" = "--display" ] && [ "$2" = "--verbose=4" ]; then
  printf '%s\n' 'Identifier=com.openai.codex' 'TeamIdentifier=2DC432GLL2' 'Authority=Developer ID Application: fixture'
fi
exit 0
"##,
        )
        .expect("codesign");
        fs::write(
            fake_bin.join("spctl"),
            r##"#!/bin/sh
printf 'spctl %s\n' "$*" >> "$INCODEX_DOCTOR_COMMAND_LOG"
exit 0
"##,
        )
        .expect("spctl");
        for name in ["codesign", "spctl"] {
            let path = fake_bin.join(name);
            let mut permissions = fs::metadata(&path).expect("metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).expect("permissions");
        }
        Self { root, app, log }
    }

    fn path(&self) -> String {
        let fake_bin = self.root.join("bin");
        let old = std::env::var("PATH").unwrap_or_default();
        format!("{}:{old}", fake_bin.display())
    }

    fn command_lines(&self) -> Vec<String> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn command(&self, name: &str) -> Vec<String> {
        self.command_lines()
            .into_iter()
            .filter(|line| line.starts_with(name))
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn run_fixture(args: &[&str], fixture: &Fixture, home: &Path) -> (i32, String, String) {
    let output = std::process::Command::new(readonly_support::bin())
        .args(args)
        .env("HOME", home)
        .env("PATH", fixture.path())
        .env("INCODEX_DOCTOR_COMMAND_LOG", &fixture.log)
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .output()
        .expect("spawn incodex");
    (
        output.status.code().unwrap_or(1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn deep_is_a_doctor_only_flag_and_is_documented() {
    let home = isolated_home();
    let (status, stdout, stderr) = run(&["doctor", "--help"], &home);
    assert_eq!(status, 0);
    assert!(stdout.contains("doctor [--json] [--deep]"), "{stdout}");
    assert!(stdout.contains("--deep"), "{stdout}");
    assert_eq!(stderr, "");

    let (status, stdout, stderr) = run(&["status", "--deep"], &home);
    assert_eq!(status, 1);
    assert_eq!(stdout, "");
    assert!(
        stderr.contains("--deep is only valid for doctor"),
        "{stderr}"
    );
}

#[test]
fn status_skips_signing_inventory_and_gatekeeper() {
    let home = isolated_home();
    let fixture = Fixture::new();
    let app = fixture.app.to_str().expect("app");
    let (status, stdout, stderr) =
        run_fixture(&["status", "--json", "--app", app], &fixture, &home);
    assert_eq!(status, 0, "{stderr}");
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_eq!(report["signing"]["status"], "not-requested");
    assert_eq!(report["spctl"]["status"], "not-requested");
    assert!(fixture.command("codesign").is_empty());
    assert!(fixture.command("spctl").is_empty());

    let (status, stdout, stderr) = run_fixture(&["status", "--app", app], &fixture, &home);
    assert_eq!(status, 0, "{stderr}");
    assert!(stdout.starts_with("➤ Status\n"));
    assert!(!stdout.contains("Gatekeeper   "));
}

#[test]
fn default_doctor_checks_only_outer_signing_and_deep_expands_inventory() {
    let home = isolated_home();
    let fixture = Fixture::new();
    let app = fixture.app.to_str().expect("app");

    let (status, stdout, stderr) =
        run_fixture(&["doctor", "--json", "--app", app], &fixture, &home);
    assert_eq!(status, 0, "{stderr}");
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_eq!(report["signing"]["status"], "not-requested");
    assert_eq!(report["spctl"]["status"], "not-requested");
    let shallow_commands = fixture.command("codesign");
    assert!(!shallow_commands.is_empty());
    assert!(shallow_commands.iter().all(|line| !line.contains("--deep")));
    assert!(shallow_commands
        .iter()
        .all(|line| !line.contains("--entitlements")));
    assert!(fixture.command("spctl").is_empty());

    let (status, stdout, stderr) = run_fixture(&["doctor", "--app", app], &fixture, &home);
    assert_eq!(status, 0, "{stderr}");
    assert!(stdout.contains("➤ App\n"));
    assert!(stdout.contains("Nested       unknown"));
    assert!(!stdout.contains("Hardened"));
    assert!(!stdout.contains("Gatekeeper   "));

    fs::write(&fixture.log, "").expect("reset command log");
    let (status, stdout, stderr) = run_fixture(
        &["doctor", "--deep", "--json", "--app", app],
        &fixture,
        &home,
    );
    assert_eq!(status, 0, "{stderr}");
    assert_eq!(stderr, "");
    let report = parse_json(&stdout);
    assert_ne!(report["signing"]["status"], "not-requested");
    assert!(!fixture.command("codesign").is_empty());
    assert!(fixture
        .command("codesign")
        .iter()
        .any(|line| line.contains("--deep")));
    assert!(fixture
        .command("codesign")
        .iter()
        .any(|line| line.contains("--entitlements")));
    assert_eq!(fixture.command("spctl").len(), 1);

    let (status, stdout, stderr) =
        run_fixture(&["doctor", "--deep", "--app", app], &fixture, &home);
    assert_eq!(status, 0, "{stderr}");
    assert!(stdout.contains("Hardened"));
    assert!(stdout.contains("Gatekeeper"));
}
