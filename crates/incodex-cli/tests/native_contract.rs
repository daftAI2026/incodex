use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_asar::pack_dir;
use incodex_macos::ditto;
use incodex_transaction::Engine;

#[path = "support/native_tty.rs"]
mod native_tty;
mod support;

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
        "incodex-native-contract-{label}-{}-{seq}",
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

fn run_rust(args: &[&str], home: &Path) -> CliResult {
    run(rust_bin(), &[], args, home)
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
        plist("com.example.incodex-native-contract", "ChatGPT"),
    )
    .unwrap();
    compile_executable(&contents.join("MacOS/ChatGPT"));

    let cua = contents.join("Frameworks/Codex Computer Use.app/Contents");
    fs::create_dir_all(&cua).unwrap();
    fs::write(
        cua.join("Info.plist"),
        plist(
            "com.example.incodex-native-contract.cua",
            "Codex Computer Use",
        ),
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
