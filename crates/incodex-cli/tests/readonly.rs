use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use incodex_asar::{pack_dir, patch_asar, MARKER_KEY};
use incodex_macos::ditto;
use incodex_transaction::Engine;
use sha2::{Digest, Sha256};

mod support;

static HOME_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_incodex")
}

fn isolated_home() -> PathBuf {
    let sequence = HOME_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("incodex-ro-{}-{n}-{sequence}", std::process::id()));
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

fn run_with_stdout_redirected(args: &[&str], home: &std::path::Path) -> (i32, String, String) {
    let _pty_gate = support::tty::acquire();
    let stdout_path = home.join("redirected.out");
    let script = r#"
import os, pty, select, sys, time
program, home, stdout_path, *args = sys.argv[1:]
env = os.environ.copy()
env["HOME"] = home
env["TERM"] = "xterm-256color"
env["NO_COLOR"] = "1"
pid, fd = pty.fork()
if pid == 0:
    output = os.open(stdout_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    os.dup2(output, 1)
    os.close(output)
    os.execvpe(program, [program, *args], env)
buf = bytearray()
deadline = time.time() + 5
status = 1
while time.time() < deadline:
    ready, _, _ = select.select([fd], [], [], 0.1)
    if ready:
        try:
            chunk = os.read(fd, 8192)
        except OSError:
            chunk = b""
        if chunk:
            buf.extend(chunk)
    done, wait_status = os.waitpid(pid, os.WNOHANG)
    if done == pid:
        status = os.waitstatus_to_exitcode(wait_status)
        break
else:
    os.kill(pid, 9)
    os.waitpid(pid, 0)
    status = 124
sys.stdout.buffer.write(("STATUS %d\n" % status).encode())
sys.stdout.buffer.write(bytes(buf))
"#;
    let output = Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(bin())
        .arg(home)
        .arg(&stdout_path)
        .args(args)
        .output()
        .expect("spawn redirected PTY harness");
    let raw = String::from_utf8_lossy(&output.stdout).into_owned();
    let (status, stderr) = raw.split_once('\n').unwrap_or((&raw, ""));
    (
        status
            .strip_prefix("STATUS ")
            .and_then(|value| value.parse().ok())
            .unwrap_or(1),
        fs::read_to_string(stdout_path).unwrap_or_default(),
        stderr.to_string(),
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
    "journalRecords",
    "checks",
    "findings",
];

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
  incodex open [--dry-run] [--app <path>]

Open an incognito window without patching Codex. Uses an isolated CODEX_HOME
and Chromium user-data-dir. The hat-glasses control and banner still appear
in that window. Closing the window burns that session.

Examples:
  incodex open
  incodex open --dry-run
",
        ),
        (
            "update",
            "\
Usage:
  incodex update [--dry-run]

Update the CLI. Script installs re-run install.sh. Homebrew installs should
use brew upgrade incodex. Source checkouts should git pull.

Examples:
  incodex update
  incodex update --dry-run
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
fn doctor_json_names_interrupted_journals() {
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
    assert_eq!(txs[0]["action"], "rollback");
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
