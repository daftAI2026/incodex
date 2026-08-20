use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_asar::{pack_dir, Archive, LOADER_NAME, MARKER_KEY};
use incodex_macos::ditto;
use incodex_transaction::Engine;

static SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, PartialEq, Eq)]
struct CliResult {
    status: i32,
    stdout: String,
    stderr: String,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn rust_bin() -> &'static str {
    env!("CARGO_BIN_EXE_incodex")
}

fn scratch(label: &str) -> PathBuf {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "incodex-parity-{label}-{}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn run(program: &str, prefix: &[&str], args: &[&str], home: &Path) -> CliResult {
    let output = Command::new(program)
        .args(prefix)
        .args(args)
        .current_dir(root())
        .env("HOME", home)
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .env("SHELL", "/bin/zsh")
        .output()
        .unwrap_or_else(|err| panic!("spawn {program}: {err}"));
    CliResult {
        status: output.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn run_ts(args: &[&str], home: &Path) -> CliResult {
    run("bun", &["src/cli.ts"], args, home)
}

fn run_rust(args: &[&str], home: &Path) -> CliResult {
    run(rust_bin(), &[], args, home)
}

fn run_tty(
    program: &str,
    prefix: &[&str],
    args: &[&str],
    home: &Path,
    wait_for: &str,
    keys: &str,
) -> CliResult {
    let script = r#"
import os, pty, select, sys, time
home, wait_for, keys = sys.argv[1], sys.argv[2].encode("utf-8"), sys.argv[3].encode("latin-1")
program = sys.argv[4]
argv = sys.argv[4:]
env = os.environ.copy()
env["HOME"] = home
env["TERM"] = "xterm-256color"
env["NO_COLOR"] = "1"
env["SHELL"] = "/bin/zsh"
pid, fd = pty.fork()
if pid == 0:
    os.chdir(env["INCODEX_TEST_ROOT"])
    os.execvpe(program, argv, env)
buf = bytearray()
sent = False
deadline = time.time() + 12
while time.time() < deadline:
    ready, _, _ = select.select([fd], [], [], 0.1)
    if not ready:
        continue
    try:
        chunk = os.read(fd, 8192)
    except OSError:
        break
    if not chunk:
        break
    buf.extend(chunk)
    if not sent and wait_for in buf:
        os.write(fd, keys)
        sent = True
try:
    _, status = os.waitpid(pid, 0)
except ChildProcessError:
    status = 0
code = os.waitstatus_to_exitcode(status) if hasattr(os, "waitstatus_to_exitcode") else 1
sys.stdout.buffer.write(("STATUS %d\n" % code).encode())
sys.stdout.buffer.write(bytes(buf))
"#;
    let mut command = Command::new("python3");
    command
        .arg("-c")
        .arg(script)
        .arg(home)
        .arg(wait_for)
        .arg(keys)
        .arg(program)
        .args(prefix)
        .args(args)
        .current_dir(root())
        .env("INCODEX_TEST_ROOT", root());
    let output = command.output().expect("spawn PTY harness");
    let raw = String::from_utf8_lossy(&output.stdout).into_owned();
    let (status_line, stdout) = raw.split_once('\n').unwrap_or((&raw, ""));
    CliResult {
        status: status_line
            .strip_prefix("STATUS ")
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(1),
        stdout: stdout.to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn visible(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if code.is_ascii() && ('@'..='~').contains(&code) {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    out.replace('\r', "")
}

fn count(text: &str, needle: &str) -> usize {
    text.match_indices(needle).count()
}

fn normalize_rust_error(text: &str) -> String {
    text.strip_prefix("  ✗ ").unwrap_or(text).to_string()
}

fn assert_menu_order(text: &str, expected: &[&str]) {
    let mut previous = 0;
    for item in expected.iter().copied() {
        let position = text
            .find(item)
            .unwrap_or_else(|| panic!("menu missing {item:?}: {text}"));
        assert!(position >= previous, "menu item order changed: {text}");
        previous = position;
    }
}

fn marker_app(home: &Path) -> PathBuf {
    let app = home.join("Marker.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "do-not-touch\n").unwrap();
    app
}

fn sleeping_open_app(home: &Path) -> PathBuf {
    let app = home.join("ChatGPT.app");
    let macos = app.join("Contents/MacOS");
    fs::create_dir_all(&macos).unwrap();
    let executable = macos.join("ChatGPT");
    fs::write(&executable, "#!/bin/sh\nsleep 0.8\n").unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).unwrap();
    app
}

fn normalize_paths(text: &str, home: &Path) -> String {
    text.replace(&home.display().to_string(), "<HOME>")
        .lines()
        .filter(|line| *line != "  ! ChatGPT is running. Install will quit it.")
        .collect::<Vec<_>>()
        .join("\n")
        + if text.ends_with('\n') { "\n" } else { "" }
}

fn normalize_diagnosis_text(text: &str, home: &Path) -> String {
    normalize_paths(text, home)
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            !line.starts_with("Target ") && !line.starts_with("Install id ")
        })
        .collect::<Vec<_>>()
        .join("\n")
        + if text.ends_with('\n') { "\n" } else { "" }
}

fn compile_executable(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let source = path.with_extension("c");
    fs::write(&source, "int main(void) { return 0; }\n").unwrap();
    let status = Command::new("cc")
        .args(["-x", "c"])
        .arg(&source)
        .arg("-o")
        .arg(path)
        .status()
        .expect("compile fixture executable");
    assert!(status.success());
    let _ = fs::remove_file(source);
}

fn plist(bundle_id: &str, executable: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>{bundle_id}</string>
  <key>CFBundleName</key><string>ChatGPT</string>
  <key>CFBundleShortVersionString</key><string>1.2.3</string>
  <key>CFBundleVersion</key><string>123</string>
  <key>CFBundleExecutable</key><string>{executable}</string>
</dict></plist>
"#
    )
}

fn patchable_app(home: &Path) -> PathBuf {
    let app = home.join("bundle/ChatGPT.app");
    let contents = app.join("Contents");
    fs::create_dir_all(contents.join("Resources")).unwrap();
    fs::write(
        contents.join("Info.plist"),
        plist("com.example.incodex-parity", "ChatGPT"),
    )
    .unwrap();
    compile_executable(&contents.join("MacOS/ChatGPT"));

    let cua = contents.join("Frameworks/Codex Computer Use.app/Contents");
    fs::create_dir_all(&cua).unwrap();
    fs::write(
        cua.join("Info.plist"),
        plist("com.example.incodex-parity.cua", "Codex Computer Use"),
    )
    .unwrap();
    compile_executable(&cua.join("MacOS/Codex Computer Use"));
    assert!(Command::new("codesign")
        .args(["--force", "--sign", "-", "--"])
        .arg(contents.join("Frameworks/Codex Computer Use.app"))
        .status()
        .expect("sign CUA fixture")
        .success());

    let src = home.join("asar-src");
    fs::create_dir_all(src.join(".vite/build")).unwrap();
    fs::write(
        src.join("package.json"),
        format!(
            "{}\n",
            serde_json::json!({"main": ".vite/build/early-bootstrap.js"})
        ),
    )
    .unwrap();
    fs::write(
        src.join(".vite/build/early-bootstrap.js"),
        "module.exports = {};\n",
    )
    .unwrap();
    pack_dir(&src, &contents.join("Resources/app.asar")).unwrap();
    app
}

fn runtime_hashes(home: &Path) -> BTreeMap<String, String> {
    let current: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(home.join(".incodex/runtime/current.json")).expect("runtime current"),
    )
    .unwrap();
    current["files"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, hash)| (name.clone(), hash.as_str().unwrap().to_string()))
        .collect()
}

fn asar_contract(app: &Path) -> (String, String, String, String) {
    let archive = Archive::open(app.join("Contents/Resources/app.asar")).unwrap();
    let package: serde_json::Value =
        serde_json::from_slice(&archive.extract("package.json").unwrap()).unwrap();
    (
        package["main"].as_str().unwrap_or_default().to_string(),
        package[MARKER_KEY]["originalMain"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        String::from_utf8(archive.extract(LOADER_NAME).unwrap()).unwrap(),
        String::from_utf8(archive.extract(".vite/build/early-bootstrap.js").unwrap()).unwrap(),
    )
}

fn json(result: &CliResult) -> serde_json::Value {
    serde_json::from_str(&result.stdout)
        .unwrap_or_else(|err| panic!("invalid JSON ({err}): {result:?}"))
}

fn assert_hex_hash(value: &serde_json::Value, field: &str) {
    let hash = value[field]
        .as_str()
        .unwrap_or_else(|| panic!("{field} is not a string: {value}"));
    assert_eq!(hash.len(), 64, "{field}: {hash}");
    assert!(
        hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{field}: {hash}"
    );
}

fn cua_signature(app: &Path) -> String {
    let output = Command::new("codesign")
        .args(["--display", "--verbose=4", "--"])
        .arg(app.join("Contents/Frameworks/Codex Computer Use.app"))
        .output()
        .unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .lines()
    .filter(|line| line.starts_with("Identifier=") || line.starts_with("CDHash="))
    .collect::<Vec<_>>()
    .join("\n")
}

fn committed_transaction(home: &Path) -> String {
    let transactions = home.join(".incodex/transactions");
    for entry in fs::read_dir(&transactions).expect("transactions") {
        let entry = entry.unwrap();
        let path = entry.path();
        let journal_path = if path.is_dir() {
            path.join("journal.json")
        } else {
            path.clone()
        };
        if !journal_path.is_file() {
            continue;
        }
        let journal: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(journal_path).unwrap()).unwrap();
        if journal["phase"] == "COMMITTED" {
            return journal["installId"].as_str().unwrap().to_string();
        }
    }
    panic!(
        "committed transaction not found in {}",
        transactions.display()
    );
}

fn committed_journal(home: &Path, install_id: &str) -> serde_json::Value {
    let v2 = home
        .join(".incodex/transactions")
        .join(install_id)
        .join("journal.json");
    let v1 = home
        .join(".incodex/transactions")
        .join(format!("{install_id}.json"));
    let path = if v2.exists() { v2 } else { v1 };
    serde_json::from_str(&fs::read_to_string(path).expect("committed journal")).unwrap()
}

fn typescript_install_manifest(home: &Path) -> serde_json::Value {
    let installations = home.join(".incodex/installations");
    for target in fs::read_dir(installations).expect("installation targets") {
        for install in fs::read_dir(target.unwrap().path()).expect("install records") {
            let manifest = install.unwrap().path().join("manifest.json");
            if manifest.is_file() {
                return serde_json::from_str(&fs::read_to_string(manifest).unwrap()).unwrap();
            }
        }
    }
    panic!("TypeScript installation manifest not found")
}

fn package_install_id(app: &Path) -> String {
    let archive = Archive::open(app.join("Contents/Resources/app.asar")).unwrap();
    archive
        .read_package_main()
        .unwrap()
        .install_id
        .expect("package install id")
}

#[test]
fn native_tty_menu_matches_the_typescript_menu_contract() {
    let home = scratch("menu");
    let ts = run_tty("bun", &["src/cli.ts"], &[], &home, "Quit", "q");
    let rust = run_tty(rust_bin(), &[], &[], &home, "Quit", "q");
    assert_eq!(rust.status, ts.status);
    assert_eq!(rust.stderr, ts.stderr);
    let ts = visible(&ts.stdout);
    let rust = visible(&rust.stdout);
    assert_menu_order(
        &ts,
        &[
            "1. Install",
            "2. Uninstall",
            "3. Open",
            "4. Status",
            "5. Doctor",
            "6. Quit",
        ],
    );
    assert_menu_order(
        &rust,
        &[
            "1. Open",
            "2. Install",
            "3. Uninstall",
            "4. Status",
            "5. Doctor",
            "6. Quit",
        ],
    );
    for text in [
        "_____   _   _",
        "https://github.com/daftAI2026/incodex",
        "Incognito toggle for Codex desktop.",
        "4. Status",
        "5. Doctor",
        "6. Quit",
        "↑↓ | Enter | V Version | Q Quit | 1-6 Jump",
    ] {
        assert!(ts.contains(text), "TS menu lost {text:?}: {ts}");
        assert!(rust.contains(text), "Rust menu missing {text:?}: {rust}");
    }
}

#[test]
fn native_menu_shows_the_same_cached_update_notice_and_shortcut_as_typescript() {
    let ts_home = scratch("menu-update-ts");
    let rust_home = scratch("menu-update-rust");
    for home in [&ts_home, &rust_home] {
        let cache = home.join(".incodex/cache/update_message");
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(&cache, "Update 9.9.9 available, run incodex update\n").unwrap();
    }
    let rust_install = rust_home.join("prefix/bin/incodex");
    fs::create_dir_all(rust_install.parent().unwrap()).unwrap();
    fs::copy(rust_bin(), &rust_install).unwrap();
    let ts = run_tty("bun", &["src/cli.ts"], &[], &ts_home, "Quit", "q");
    let rust = run_tty(
        rust_install.to_str().unwrap(),
        &[],
        &[],
        &rust_home,
        "Quit",
        "q",
    );
    let ts = visible(&ts.stdout);
    let rust = visible(&rust.stdout);
    for text in [
        "Update 9.9.9 available, run incodex update",
        "↑↓ | Enter | U Update | V Version | Q Quit | 1-6 Jump",
    ] {
        assert!(ts.contains(text), "TS menu lost {text:?}: {ts}");
        assert!(rust.contains(text), "Rust menu missing {text:?}: {rust}");
    }
}

#[test]
fn native_open_animates_while_waiting_for_cdp_readiness_and_clears_its_line() {
    let home = scratch("open-spinner");
    let app = sleeping_open_app(&home);
    let args = ["open", "--app", app.to_str().unwrap()];
    let rust = run_tty(
        rust_bin(),
        &[],
        &args,
        &home,
        "Closed. Isolated session removed.",
        "",
    );
    assert_eq!(rust.status, 0, "{}", rust.stdout);
    assert!(
        rust.stdout.contains("Waiting for Codex UI to become ready"),
        "missing opening readiness animation: {}",
        visible(&rust.stdout)
    );
    assert!(
        ["|", "/", "-", "\\"].iter().any(|frame| rust
            .stdout
            .contains(&format!("  {frame} Waiting for Codex UI to become ready"))),
        "missing spinner frames: {:?}",
        rust.stdout
    );
    assert!(
        rust.stdout.contains("\r\u{1b}[2K"),
        "spinner must clear the current line: {:?}",
        rust.stdout
    );
}

#[test]
fn native_tty_uninstall_animates_immediately_after_confirmation() {
    let home = scratch("uninstall-progress-tty");
    let app = patchable_app(&home);
    let install = run_rust(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(install.status, 0, "{install:?}");

    let rust = run_tty(
        rust_bin(),
        &[],
        &["uninstall", "--app", app.to_str().unwrap()],
        &home,
        "Press Enter to confirm, ESC to cancel: ",
        "\r",
    );
    assert_eq!(rust.status, 0, "{}", visible(&rust.stdout));
    assert!(
        ["|", "/", "-", "\\"].iter().any(|frame| rust
            .stdout
            .contains(&format!("  {frame} Restoring original app"))),
        "confirmation was followed by a silent uninstall: {:?}",
        rust.stdout
    );
    assert!(
        rust.stdout.contains("\r\u{1b}[2K"),
        "uninstall spinner must clear the current line: {:?}",
        rust.stdout
    );
    assert!(
        visible(&rust.stdout).contains("Official app restored. Dock was refreshed."),
        "missing final uninstall result: {}",
        visible(&rust.stdout)
    );
}

#[test]
fn native_tty_install_animates_immediately_after_confirmation() {
    let home = scratch("install-progress-tty");
    let app = patchable_app(&home);
    let rust = run_tty(
        rust_bin(),
        &[],
        &["install", "--app", app.to_str().unwrap()],
        &home,
        "Press Enter to confirm, ESC to cancel: ",
        "\r",
    );
    assert_eq!(rust.status, 0, "{}", visible(&rust.stdout));
    assert!(
        ["|", "/", "-", "\\"].iter().any(|frame| rust
            .stdout
            .contains(&format!("  {frame} Backing up original app"))),
        "confirmation was followed by a silent install: {:?}",
        rust.stdout
    );
    assert!(
        rust.stdout.contains("\r\u{1b}[2K"),
        "install spinner must clear the current line: {:?}",
        rust.stdout
    );
    assert!(
        ["|", "/", "-", "\\"].iter().any(|frame| rust
            .stdout
            .contains(&format!("  {frame} Replacing application"))),
        "install should expose a product phase instead of a transaction primitive: {:?}",
        rust.stdout
    );
    for internal in ["Preparing installation transaction", "Swapping application"] {
        assert!(
            !rust.stdout.contains(internal),
            "TTY leaked internal phase {internal:?}: {:?}",
            rust.stdout
        );
    }
}

#[test]
fn native_tty_failure_clears_progress_before_printing_the_error() {
    let home = scratch("failed-progress-tty");
    let app = patchable_app(&home);
    let rust = run_tty(
        rust_bin(),
        &[],
        &["uninstall", "--app", app.to_str().unwrap()],
        &home,
        "Press Enter to confirm, ESC to cancel: ",
        "\r",
    );
    assert_eq!(rust.status, 1, "{}", visible(&rust.stdout));
    let clear = rust
        .stdout
        .rfind("\r\u{1b}[2K")
        .expect("failed progress must clear its current line");
    let error = rust
        .stdout
        .rfind("no installation record for this target")
        .expect("missing explicit uninstall error");
    assert!(
        clear < error,
        "error was printed before progress cleanup: {:?}",
        rust.stdout
    );
    assert!(
        visible(&rust.stdout).contains("  ✗ no installation record for this target"),
        "error should follow the CLI body indentation and mark: {}",
        visible(&rust.stdout)
    );
}

#[test]
fn native_tty_runtime_animates_and_clears_its_line() {
    let home = scratch("runtime-progress-tty");
    let rust = run_tty(
        rust_bin(),
        &[],
        &["runtime"],
        &home,
        "Runtime updated. Codex was not modified.",
        "",
    );
    assert_eq!(rust.status, 0, "{}", visible(&rust.stdout));
    assert!(
        ["|", "/", "-", "\\"].iter().any(|frame| rust
            .stdout
            .contains(&format!("  {frame} Publishing Runtime"))),
        "runtime publish was silent: {:?}",
        rust.stdout
    );
    assert!(
        rust.stdout.contains("\r\u{1b}[2K"),
        "runtime spinner must clear the current line: {:?}",
        rust.stdout
    );
}

#[test]
fn native_tty_status_and_doctor_animate_without_changing_machine_output() {
    for (command, stage, result) in [
        ("status", "Inspecting installation status", "➤ Status"),
        ("doctor", "Running diagnostics", "➤ App"),
    ] {
        let home = scratch(&format!("{command}-progress-tty"));
        let app = marker_app(&home);
        let rust = run_tty(
            rust_bin(),
            &[],
            &[command, "--app", app.to_str().unwrap()],
            &home,
            result,
            "",
        );
        assert_eq!(rust.status, 0, "{command}: {}", visible(&rust.stdout));
        assert!(
            ["|", "/", "-", "\\"]
                .iter()
                .any(|frame| rust.stdout.contains(&format!("  {frame} {stage}"))),
            "{command} was silent: {:?}",
            rust.stdout
        );
        assert!(
            rust.stdout.contains("\r\u{1b}[2K"),
            "{command} spinner did not clear: {:?}",
            rust.stdout
        );

        let json = run_rust(&[command, "--json", "--app", app.to_str().unwrap()], &home);
        assert_eq!(json.status, 0, "{json:?}");
        assert!(
            serde_json::from_str::<serde_json::Value>(&json.stdout).is_ok(),
            "{command} progress corrupted JSON: {json:?}"
        );
        assert!(!json.stdout.contains(stage), "{json:?}");
    }
}

#[test]
fn native_tty_recover_animates_until_the_transaction_is_restored() {
    let home = scratch("recover-progress-tty");
    let app = patchable_app(&home);
    assert!(
        Command::new("codesign")
            .args(["--force", "--deep", "--sign", "-", "--"])
            .arg(&app)
            .status()
            .unwrap()
            .success(),
        "fixture must start with a verifiable signature"
    );
    let root = home.join(".incodex");
    let mut transaction = Engine::begin(&root, &app, "install").unwrap();
    let id = transaction.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    ditto(&app, &original).unwrap();
    transaction.mark_backup_committed().unwrap();
    let staged = root
        .join("scratch")
        .join(format!("ChatGPT.app.staged-{id}"));
    ditto(&app, &staged).unwrap();
    transaction.place_staging(&staged).unwrap();
    transaction.swap().unwrap();
    drop(transaction);

    let rust = run_tty(
        rust_bin(),
        &[],
        &["recover", "--transaction", &id],
        &home,
        "outgoing restored: true",
        "",
    );
    assert_eq!(rust.status, 0, "{}", visible(&rust.stdout));
    assert!(
        ["|", "/", "-", "\\"].iter().any(|frame| rust
            .stdout
            .contains(&format!("  {frame} Recovering transaction"))),
        "recover was silent: {:?}",
        rust.stdout
    );
    assert!(
        rust.stdout.contains("\r\u{1b}[2K"),
        "recover spinner must clear the current line: {:?}",
        rust.stdout
    );
}

#[test]
fn native_tty_self_uninstall_animates_while_removing_the_cli() {
    let home = scratch("self-uninstall-progress-tty");
    let bin = home.join("prefix/bin");
    fs::create_dir_all(&bin).unwrap();
    let installed = bin.join("incodex");
    fs::copy(rust_bin(), &installed).unwrap();
    fs::copy(rust_bin(), bin.join("inc")).unwrap();

    let rust = run_tty(
        installed.to_str().unwrap(),
        &[],
        &["self-uninstall"],
        &home,
        "Press Enter to confirm, ESC to cancel: ",
        "\r",
    );
    assert_eq!(rust.status, 0, "{}", visible(&rust.stdout));
    assert!(
        ["|", "/", "-", "\\"].iter().any(|frame| rust
            .stdout
            .contains(&format!("  {frame} Removing Incodex CLI"))),
        "self-uninstall was silent: {:?}",
        rust.stdout
    );
    assert!(
        rust.stdout.contains("\r\u{1b}[2K"),
        "self-uninstall spinner must clear the current line: {:?}",
        rust.stdout
    );
    assert!(!installed.exists());
    assert!(!bin.join("inc").exists());
}

#[test]
fn native_non_tty_mutations_print_auditable_progress_stages() {
    let home = scratch("mutation-progress-non-tty");
    let app = patchable_app(&home);
    let install = run_rust(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(install.status, 0, "{install:?}");
    assert!(
        install.stdout.contains("➤ Publishing Runtime")
            && install.stdout.contains("➤ Backing up original app")
            && install.stdout.contains("➤ Patching and signing app")
            && install.stdout.contains("➤ Replacing application")
            && install.stdout.contains("➤ Verifying installation"),
        "install must expose durable stages without TTY controls: {install:?}"
    );
    assert!(!install.stdout.contains('\u{1b}'), "{install:?}");

    let uninstall = run_rust(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &home,
    );
    assert_eq!(uninstall.status, 0, "{uninstall:?}");
    assert!(
        uninstall.stdout.contains("➤ Restoring original app"),
        "uninstall must expose its active stage: {uninstall:?}"
    );
    assert!(!uninstall.stdout.contains('\u{1b}'), "{uninstall:?}");
}

#[test]
fn native_lifecycle_commands_match_the_typescript_source_checkout_contract() {
    let ts_home = scratch("lifecycle-ts");
    let rust_home = scratch("lifecycle-rust");
    for args in [
        &["runtime", "--dry-run"][..],
        &["update", "--dry-run"],
        &["self-uninstall", "--dry-run"],
    ] {
        let ts = run_ts(args, &ts_home);
        let rust = run_rust(args, &rust_home);
        assert_eq!(rust.status, ts.status, "status parity failed for {args:?}");
        assert_eq!(rust.stdout, ts.stdout, "stdout parity failed for {args:?}");
        assert_eq!(
            normalize_rust_error(&rust.stderr),
            ts.stderr,
            "stderr parity failed for {args:?}"
        );
    }
}

#[test]
fn native_tty_install_and_uninstall_ask_once_and_escape_aborts() {
    for command in ["install", "uninstall"] {
        let home = scratch(command);
        let app = marker_app(&home);
        let args = [command, "--app", app.to_str().unwrap()];
        let rust = run_tty(
            rust_bin(),
            &[],
            &args,
            &home,
            "Press Enter to confirm, ESC to cancel: ",
            "\u{1b}",
        );
        let output = visible(&rust.stdout);
        assert_eq!(
            count(&output, "Press Enter to confirm, ESC to cancel: "),
            1,
            "{command}: {output}"
        );
        assert!(output.contains("aborted"), "{command}: {output}");
        assert_eq!(
            fs::read_to_string(app.join("marker")).unwrap(),
            "do-not-touch\n"
        );
    }
}

#[test]
fn plans_and_non_tty_refusals_match_on_the_same_fixture() {
    let ts_home = scratch("plan-ts");
    let rs_home = scratch("plan-rs");
    let ts_app = marker_app(&ts_home);
    let rs_app = marker_app(&rs_home);
    let cases: &[&[&str]] = &[
        &["install", "--dry-run", "--app", "APP"],
        &["uninstall", "--dry-run", "--app", "APP"],
        &["open", "--dry-run", "--app", "APP"],
        &["install", "--app", "APP"],
        &["uninstall", "--app", "APP"],
        &["recover", "--transaction", "does-not-exist"],
    ];
    for case in cases {
        let ts_args: Vec<&str> = case
            .iter()
            .map(|value| {
                if *value == "APP" {
                    ts_app.to_str().unwrap()
                } else {
                    value
                }
            })
            .collect();
        let rs_args: Vec<&str> = case
            .iter()
            .map(|value| {
                if *value == "APP" {
                    rs_app.to_str().unwrap()
                } else {
                    value
                }
            })
            .collect();
        let ts = run_ts(&ts_args, &ts_home);
        let rust = run_rust(&rs_args, &rs_home);
        assert_eq!(
            rust.status, ts.status,
            "case={case:?}\nTS={ts:?}\nRust={rust:?}"
        );
        assert_eq!(
            normalize_paths(&rust.stdout, &rs_home),
            normalize_paths(&ts.stdout, &ts_home),
            "stdout case={case:?}"
        );
        assert_eq!(
            normalize_paths(&normalize_rust_error(&rust.stderr), &rs_home),
            normalize_paths(&ts.stderr, &ts_home),
            "stderr case={case:?}"
        );
    }
}

#[test]
fn install_uninstall_artifacts_match_on_the_same_fixture() {
    let ts_home = scratch("live-ts");
    let rs_home = scratch("live-rs");
    let ts_app = patchable_app(&ts_home);
    let rs_app = patchable_app(&rs_home);
    let ts_original = fs::read(ts_app.join("Contents/Resources/app.asar")).unwrap();
    let rs_original = fs::read(rs_app.join("Contents/Resources/app.asar")).unwrap();
    assert_eq!(ts_original, rs_original);
    let ts_cua = cua_signature(&ts_app);
    let rs_cua = cua_signature(&rs_app);
    assert_eq!(ts_cua, rs_cua);

    let ts_install = run_ts(
        &["install", "--yes", "--app", ts_app.to_str().unwrap()],
        &ts_home,
    );
    let rs_install = run_rust(
        &["install", "--yes", "--app", rs_app.to_str().unwrap()],
        &rs_home,
    );
    assert_eq!(ts_install.status, 0, "{ts_install:?}");
    assert_eq!(rs_install.status, 0, "{rs_install:?}");
    assert_eq!(asar_contract(&ts_app), asar_contract(&rs_app));
    assert_eq!(runtime_hashes(&ts_home), runtime_hashes(&rs_home));
    assert_eq!(cua_signature(&ts_app), ts_cua);
    assert_eq!(cua_signature(&rs_app), rs_cua);
    for app in [&ts_app, &rs_app] {
        assert!(
            Command::new("codesign")
                .args(["--verify", "--deep", "--strict", "--"])
                .arg(app)
                .status()
                .unwrap()
                .success(),
            "host must verify: {}",
            app.display()
        );
    }

    let ts_status = run_ts(
        &["status", "--json", "--app", ts_app.to_str().unwrap()],
        &ts_home,
    );
    let rs_status = run_rust(
        &["status", "--json", "--app", rs_app.to_str().unwrap()],
        &rs_home,
    );
    assert_eq!(ts_status.status, 0, "{ts_status:?}");
    assert_eq!(rs_status.status, 0, "{rs_status:?}");
    let ts_diagnosis = json(&ts_status);
    let rs_diagnosis = json(&rs_status);
    for field in [
        "exists",
        "patched",
        "bundleId",
        "appVersion",
        "appBuild",
        "architecture",
        "runtimeVersion",
        "originalMain",
        "codesignOk",
        "asarLoaderOnly",
    ] {
        assert_eq!(
            rs_diagnosis[field], ts_diagnosis[field],
            "diagnosis field {field}\nTS={ts_diagnosis}\nRust={rs_diagnosis}"
        );
    }
    let runtime_version = env!("CARGO_PKG_VERSION");
    let runtime_release = format!("releases/{runtime_version}");
    for diagnosis in [&ts_diagnosis, &rs_diagnosis] {
        for field in [
            "asarFileHash",
            "asarHeaderHash",
            "plistFileHash",
            "plistIntegrityHash",
        ] {
            assert_hex_hash(diagnosis, field);
        }
        assert!(diagnosis["backup"].is_object(), "backup: {diagnosis}");
        assert_eq!(diagnosis["externalRuntime"]["present"], true);
        assert_eq!(diagnosis["externalRuntime"]["ok"], true);
        assert_eq!(diagnosis["externalRuntime"]["version"], runtime_version);
        assert_eq!(diagnosis["externalRuntime"]["release"], runtime_release);
        assert!(diagnosis["signing"].is_object(), "signing: {diagnosis}");
        assert!(diagnosis["spctl"].is_object(), "spctl: {diagnosis}");
    }

    for command in ["status", "doctor"] {
        let ts = run_ts(&[command, "--app", ts_app.to_str().unwrap()], &ts_home);
        let rust = run_rust(&[command, "--app", rs_app.to_str().unwrap()], &rs_home);
        assert_eq!(
            rust.status, ts.status,
            "{command}\nTS={ts:?}\nRust={rust:?}"
        );
        assert_eq!(rust.stderr, ts.stderr, "{command}");
        assert_eq!(
            normalize_diagnosis_text(&rust.stdout, &rs_home),
            normalize_diagnosis_text(&ts.stdout, &ts_home),
            "{command} output\nTS={ts:?}\nRust={rust:?}"
        );
    }

    let ts_id = committed_transaction(&ts_home);
    let rs_id = committed_transaction(&rs_home);
    let ts_manifest = typescript_install_manifest(&ts_home);
    let ts_journal = committed_journal(&ts_home, &ts_id);
    let rs_journal = committed_journal(&rs_home, &rs_id);
    assert_eq!(package_install_id(&ts_app), ts_id);
    assert_eq!(package_install_id(&rs_app), rs_id);
    assert_eq!(ts_manifest["installId"], ts_id);
    assert_eq!(ts_manifest["transactionState"], "committed");
    assert_eq!(ts_journal["phase"], "COMMITTED");
    assert_eq!(rs_journal["schemaVersion"], 2);
    assert_eq!(rs_journal["phase"], "COMMITTED");
    assert_eq!(
        rs_journal["target"]["realPath"],
        rs_app.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(
        ts_manifest["runtimeVersion"],
        rs_diagnosis["runtimeVersion"]
    );
    for field in ["bundleIdentifier", "appVersion", "appBuild", "architecture"] {
        let diagnosis_field = if field == "bundleIdentifier" {
            "bundleId"
        } else {
            field
        };
        assert_eq!(ts_manifest[field], ts_diagnosis[diagnosis_field]);
        assert_eq!(ts_manifest[field], rs_diagnosis[diagnosis_field]);
    }
    assert_eq!(
        ts_manifest["originalAsarFileHash"],
        rs_diagnosis["backup"]["originalAsarFileHash"]
    );
    assert_eq!(
        ts_manifest["patchedAsarFileHash"],
        ts_diagnosis["asarFileHash"]
    );
    assert_eq!(
        rs_diagnosis["backup"]["patchedAsarFileHash"],
        rs_diagnosis["asarFileHash"]
    );
    let ts_recover = run_ts(&["recover", "--transaction", &ts_id], &ts_home);
    let rs_recover = run_rust(&["recover", "--transaction", &rs_id], &rs_home);
    assert_eq!(ts_recover.status, 0, "{ts_recover:?}");
    assert_eq!(rs_recover.status, 0, "{rs_recover:?}");
    for line in [
        "phase: COMMITTED",
        "action: done",
        "target present: true",
        "backup intact: true",
        "staged removed: true",
        "outgoing restored: false",
    ] {
        assert!(
            ts_recover.stdout.contains(line),
            "TS recover: {ts_recover:?}"
        );
        assert!(
            rs_recover.stdout.contains(line),
            "Rust recover: {rs_recover:?}"
        );
    }

    let ts_uninstall = run_ts(
        &["uninstall", "--yes", "--app", ts_app.to_str().unwrap()],
        &ts_home,
    );
    let rs_uninstall = run_rust(
        &["uninstall", "--yes", "--app", rs_app.to_str().unwrap()],
        &rs_home,
    );
    assert_eq!(ts_uninstall.status, 0, "{ts_uninstall:?}");
    assert_eq!(rs_uninstall.status, 0, "{rs_uninstall:?}");
    assert_eq!(
        fs::read(ts_app.join("Contents/Resources/app.asar")).unwrap(),
        ts_original
    );
    assert_eq!(
        fs::read(rs_app.join("Contents/Resources/app.asar")).unwrap(),
        rs_original
    );
}
