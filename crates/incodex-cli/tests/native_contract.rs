use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_asar::pack_dir;
use incodex_macos::ditto;
use incodex_transaction::Engine;
use sha2::{Digest, Sha256};

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
    fs::write(
        app.join("Contents/Info.plist"),
        plist("com.example.incodex-open", "ChatGPT"),
    )
    .unwrap();
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

#[derive(Debug, PartialEq, Eq)]
struct RuntimeTreeEntry {
    relative: PathBuf,
    kind: u8,
    mode: u32,
    size: u64,
    content_hash: Option<[u8; 32]>,
    mtime: (i64, i64),
}

fn runtime_tree(root: &Path) -> Vec<RuntimeTreeEntry> {
    fn collect(root: &Path, current: &Path, entries: &mut Vec<RuntimeTreeEntry>) {
        let metadata = fs::symlink_metadata(current).unwrap();
        let file_type = metadata.file_type();
        let relative = if current == root {
            PathBuf::from(".")
        } else {
            current
                .strip_prefix(root)
                .unwrap_or_else(|_| panic!("runtime path escaped root: {}", current.display()))
                .to_path_buf()
        };
        let (kind, size, content_hash) = if file_type.is_dir() {
            (b'D', metadata.len(), None)
        } else if file_type.is_symlink() {
            let target = fs::read_link(current).unwrap();
            (
                b'L',
                target.as_os_str().as_bytes().len() as u64,
                Some(Sha256::digest(target.as_os_str().as_bytes()).into()),
            )
        } else if file_type.is_file() {
            let body = fs::read(current).unwrap();
            (b'F', metadata.len(), Some(Sha256::digest(body).into()))
        } else {
            (b'O', metadata.len(), None)
        };
        entries.push(RuntimeTreeEntry {
            relative,
            kind,
            mode: metadata.mode() & 0o7777,
            size,
            content_hash,
            mtime: (metadata.mtime(), metadata.mtime_nsec()),
        });
        if file_type.is_dir() {
            let mut children = fs::read_dir(current)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>();
            children.sort();
            for child in children {
                collect(root, &child, entries);
            }
        }
    }

    let mut entries = Vec::new();
    collect(root, root, &mut entries);
    entries.sort_by(|left, right| left.relative.cmp(&right.relative));
    entries
}

#[test]
fn native_non_tty_mutations_print_auditable_progress_stages() {
    let home = scratch("mutation-progress-non-tty");
    let app = patchable_app(&home);
    let install = run_rust(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(install.status, 0, "{install:?}");
    let version = install.stdout.find("  Version").unwrap();
    let checking = install
        .stdout
        .find("➤ Checking app signature")
        .expect("install plan must announce its signing preflight");
    let signed = install.stdout.find("  Signed").unwrap();
    assert!(
        version < checking && checking < signed,
        "signing preflight must be visible between Version and Signed: {install:?}"
    );
    assert!(
        install.stdout.contains("➤ Publishing Runtime")
            && install.stdout.contains("➤ Backing up original app")
            && install.stdout.contains("➤ Patching and signing app")
            && install.stdout.contains("➤ Replacing the app")
            && install.stdout.contains("➤ Verifying installation"),
        "install must expose durable stages without TTY controls: {install:?}"
    );
    assert!(!install.stdout.contains('\u{1b}'), "{install:?}");
    assert!(
        !install.stdout.contains("Codex Storage Key"),
        "a foreign custom bundle must not receive Codex-specific Keychain advice: {install:?}"
    );

    let uninstall = run_rust(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &home,
    );
    assert_eq!(uninstall.status, 0, "{uninstall:?}");
    assert!(
        uninstall.stdout.contains("➤ Restoring original app")
            && uninstall.stdout.contains("➤ Refreshing app registration")
            && uninstall
                .stdout
                .contains("App restored. App registration was refreshed."),
        "uninstall must expose its active stage: {uninstall:?}"
    );
    assert!(
        !uninstall.stdout.contains("Official app") && !uninstall.stdout.contains("Dock"),
        "custom app uninstall must not claim an official target or Dock refresh: {uninstall:?}"
    );
    assert!(!uninstall.stdout.contains('\u{1b}'), "{uninstall:?}");
}

#[test]
fn native_install_prints_keychain_advice_only_after_a_new_codex_patch() {
    let home = scratch("install-keychain-advice");
    let app = patchable_app(&home);
    fs::write(
        app.join("Contents/Info.plist"),
        plist("com.openai.codex", "ChatGPT"),
    )
    .unwrap();

    let dry_run = run_rust(
        &[
            "install",
            "--dry-run",
            "--app",
            app.to_str().unwrap(),
        ],
        &home,
    );
    assert_eq!(dry_run.status, 0, "{dry_run:?}");
    assert!(
        !dry_run.stdout.contains("Codex Storage Key"),
        "a dry run must not predict that a Keychain prompt will occur: {dry_run:?}"
    );

    let install = run_rust(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(install.status, 0, "{install:?}");
    assert!(
        install.stdout.contains("Codex Storage Key")
            && install.stdout.contains("Mac login password")
            && install.stdout.contains("not your ChatGPT password")
            && install.stdout.contains("Always Allow")
            && install.stdout.contains("Deny or Cancel"),
        "a committed Codex patch must explain the possible Keychain prompt safely: {install:?}"
    );

    let repeated = run_rust(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(repeated.status, 0, "{repeated:?}");
    assert!(
        repeated.stdout.contains("Already current")
            && !repeated.stdout.contains("Codex Storage Key"),
        "an already-current install must not repeat first-patch Keychain advice: {repeated:?}"
    );
}

#[test]
fn native_non_tty_uninstall_requires_yes_and_leaves_the_target_untouched() {
    let home = scratch("uninstall-non-tty-refusal");
    let app = marker_app(&home);
    let marker = app.join("marker");
    let before = fs::read(&marker).unwrap();

    let uninstall = run_rust(&["uninstall", "--app", app.to_str().unwrap()], &home);
    assert_eq!(uninstall.status, 1, "{uninstall:?}");
    assert!(
        uninstall.stdout.contains("➤ Uninstall")
            && uninstall
                .stdout
                .contains(&format!("  App          {}", app.display())),
        "refusal must still print the auditable plan: {uninstall:?}"
    );
    assert_eq!(
        uninstall.stderr,
        "  ✗ non-interactive uninstall requires --yes\n  incodex uninstall --yes\n"
    );
    assert_eq!(fs::read(&marker).unwrap(), before);
}

#[test]
fn native_runtime_dry_run_does_not_create_or_modify_runtime_state() {
    let missing_home = scratch("runtime-dry-run-missing");
    let missing_runtime = missing_home.join(".incodex/runtime");
    let dry_run = run_rust(&["runtime", "--dry-run"], &missing_home);
    assert_eq!(dry_run.status, 0, "{dry_run:?}");
    assert_eq!(dry_run.stderr, "");
    assert_eq!(
        dry_run.stdout,
        "would update ~/.incodex/runtime/ without modifying Codex\n"
    );
    assert!(
        !missing_runtime.exists(),
        "a dry run must not create the runtime directory"
    );

    let home = scratch("runtime-dry-run-existing");
    let runtime = home.join(".incodex/runtime");
    fs::create_dir_all(&runtime).unwrap();
    let sentinel = runtime.join("sentinel");
    fs::write(&sentinel, "keep\n").unwrap();
    let before = runtime_tree(&runtime);

    let dry_run = run_rust(&["runtime", "--dry-run"], &home);
    assert_eq!(dry_run.status, 0, "{dry_run:?}");
    assert_eq!(dry_run.stderr, "");
    assert_eq!(
        dry_run.stdout,
        "would update ~/.incodex/runtime/ without modifying Codex\n"
    );
    assert_eq!(runtime_tree(&runtime), before);
}
