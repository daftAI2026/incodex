use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_incodex")
}

fn isolated_home() -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("incodex-ro-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).expect("home");
    dir
}

fn run(args: &[&str], home: &std::path::Path) -> (i32, String, String) {
    let output = Command::new(bin())
        .args(args)
        .env("HOME", home)
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .env("SHELL", "/bin/zsh")
        .output()
        .expect("spawn incodex");
    (
        output.status.code().unwrap_or(1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn parse_json(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout.trim_end()).expect("json")
}

fn top_level_json_keys(stdout: &str) -> Vec<&str> {
    stdout
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("  \"")?;
            let (key, _) = rest.split_once("\":")?;
            Some(key)
        })
        .collect()
}

const DIAGNOSIS_KEYS: &[&str] = &[
    "target",
    "targetId",
    "exists",
    "patched",
    "bundleId",
    "appVersion",
    "appBuild",
    "architecture",
    "asarFileHash",
    "asarHeaderHash",
    "plistFileHash",
    "plistIntegrityHash",
    "runtimeVersion",
    "originalMain",
    "codesignOk",
    "backup",
    "stalePid",
    "orphanSessions",
    "leftoverChromium",
    "asarLoaderOnly",
    "externalRuntime",
    "signing",
    "spctl",
    "interruptedTransactions",
];

#[test]
fn command_help_matches_typescript() {
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
  incodex doctor [--json] [--app <path>]

Diagnose the install, runtime files, and leftover sessions.

Examples:
  incodex doctor
  incodex doctor --json
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
        format!("➤ Status\n  ! Codex app not found: {}\n\n", app.display())
    );
    assert!(!home.join(".incodex").exists());
}

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
  ! missing current.json
  Loader       unknown
  Main         unknown

➤ Signing
  Verify       failed

➤ Backup
  State        none

➤ Sessions
  Orphans      0
  Chromium     0
  Stale pid    no
  Journals     0

",
            app = app.display()
        )
    );
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
        let (status, stdout, stderr) = run(
            &[command, "--json", "--app", app.to_str().unwrap()],
            &home,
        );
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
fn doctor_json_names_interrupted_journals() {
    let home = isolated_home();
    let app = home.join("Missing.app");
    let tx_dir = home.join(".incodex").join("transactions");
    fs::create_dir_all(&tx_dir).unwrap();
    fs::write(
        tx_dir.join("tx-golden.json"),
        serde_json::json!({
            "schemaVersion": 1,
            "installId": "tx-golden",
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
    let (status, stdout, stderr) = run(
        &["doctor", "--json", "--app", app.to_str().unwrap()],
        &home,
    );
    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    let rec = parse_json(&stdout);
    let txs = rec["interruptedTransactions"].as_array().unwrap();
    assert_eq!(txs.len(), 1);
    assert_eq!(txs[0]["installId"], "tx-golden");
    assert_eq!(txs[0]["phase"], "PATCHED");
    assert_eq!(txs[0]["action"], "rollback");
}

#[test]
fn unknown_flags_fail_closed() {
    let home = isolated_home();
    let cases: &[(&[&str], &str)] = &[
        (&["wipe"], "  ✗ unknown command: wipe\n  incodex --help\n"),
        (&["status", "--please"], "  ✗ unknown flag: --please\n  incodex --help\n"),
        (&["status", "--app"], "  ✗ --app requires a path, not another flag\n"),
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
