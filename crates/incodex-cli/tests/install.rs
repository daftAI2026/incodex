use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_asar::{pack_dir, Archive, LOADER_NAME, MARKER_KEY};
use incodex_transaction::{acquire_target_lock, Engine};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_incodex")
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn isolated_home() -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("incodex-install-{}-{n}", std::process::id()));
    fs::create_dir_all(&dir).expect("home");
    dir
}

fn run(args: &[&str], home: &Path) -> (i32, String, String) {
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

fn run_with_path(args: &[&str], home: &Path, path: &Path) -> (i32, String, String) {
    let output = Command::new(bin())
        .args(args)
        .env("HOME", home)
        .env("PATH", path)
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

fn path_with_fake_bin(bin: &Path) -> PathBuf {
    let mut path = OsString::from(bin.as_os_str());
    path.push(":");
    if let Some(existing) = std::env::var_os("PATH") {
        path.push(existing);
    }
    PathBuf::from(path)
}

fn incodex_paths(home: &Path) -> Vec<String> {
    let dir = home.join(".incodex");
    if !dir.exists() {
        return Vec::new();
    }
    let mut out = Vec::new();
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            out.push(path.strip_prefix(root).unwrap().display().to_string());
            if path.is_dir() {
                walk(&path, root, out);
            }
        }
    }
    walk(&dir, &dir, &mut out);
    out.sort();
    out
}

fn install_mutations(home: &Path) -> Vec<String> {
    incodex_paths(home)
        .into_iter()
        .filter(|path| !path.starts_with("cache"))
        .collect()
}

fn marker_app(home: &Path) -> PathBuf {
    let app = home.join("Marker.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "do-not-touch\n").unwrap();
    app
}

fn write_executable(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn patchable_app(home: &Path) -> PathBuf {
    let root = home.join("bundle");
    let app = root.join("ChatGPT.app");
    let contents = app.join("Contents");
    fs::create_dir_all(contents.join("Resources")).unwrap();
    fs::write(
        contents.join("Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>com.example.incodex-fixture</string>
  <key>CFBundleName</key>
  <string>ChatGPT</string>
  <key>CFBundleShortVersionString</key>
  <string>1.2.3</string>
  <key>CFBundleVersion</key>
  <string>123</string>
  <key>CFBundleExecutable</key>
  <string>ChatGPT</string>
</dict>
</plist>
"#,
    )
    .unwrap();
    write_executable(
        &contents.join("MacOS").join("ChatGPT"),
        "#!/bin/sh\nexit 0\n",
    );
    let cua_app = contents.join("Frameworks/CUALockScreenGuardian.app");
    fs::create_dir_all(cua_app.join("Contents")).unwrap();
    fs::write(
        cua_app.join("Contents/Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.example.cua-guardian</string>
  <key>CFBundleExecutable</key><string>CUALockScreenGuardian</string>
  <key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#,
    )
    .unwrap();
    write_executable(
        &cua_app.join("Contents/MacOS/CUALockScreenGuardian"),
        "#!/bin/sh\necho vendor-helper\nexit 0\n",
    );
    let signed = Command::new("codesign")
        .args(["--force", "--sign", "-", "--"])
        .arg(&cua_app)
        .status()
        .expect("sign fixture vendor helper");
    assert!(signed.success());
    let src = home.join("asar-src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("package.json"),
        format!("{}\n", serde_json::json!({"main":"index.js"})),
    )
    .unwrap();
    fs::write(src.join("index.js"), "ok\n").unwrap();
    pack_dir(&src, &contents.join("Resources").join("app.asar")).unwrap();
    app
}

fn codesign_display(path: &Path) -> String {
    let output = Command::new("codesign")
        .args(["-d", "-v", "--"])
        .arg(path)
        .output()
        .expect("codesign");
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn is_signed(path: &Path) -> bool {
    Command::new("codesign")
        .args(["--verify", "--"])
        .arg(path)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[test]
fn install_dry_run_app_prints_plan_and_does_not_mutate() {
    let home = isolated_home();
    let app = marker_app(&home);
    let (status, stdout, stderr) = run(
        &["install", "--dry-run", "--app", app.to_str().unwrap()],
        &home,
    );
    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    assert!(stdout.contains("➤ Install"));
    assert!(stdout.contains(&format!("  App          {}", app.display())));
    assert!(stdout.contains("  ! Dry run. No files changed."));
    assert_eq!(install_mutations(&home), Vec::<String>::new());
    assert_eq!(
        fs::read_to_string(app.join("marker")).unwrap(),
        "do-not-touch\n"
    );
}

#[test]
fn install_short_n_is_the_same_as_dry_run() {
    let home = isolated_home();
    let app = marker_app(&home);
    let dashed = run(
        &["install", "--dry-run", "--app", app.to_str().unwrap()],
        &home,
    );
    let short = run(&["install", "-n", "--app", app.to_str().unwrap()], &home);
    assert_eq!(dashed.0, 0);
    assert_eq!(short, dashed);
    assert!(dashed.1.contains("  ! Dry run. No files changed."));
}

#[test]
fn uninstall_dry_run_app_matches_golden() {
    let home = isolated_home();
    let app = marker_app(&home);
    let (status, stdout, stderr) = run(
        &["uninstall", "--dry-run", "--app", app.to_str().unwrap()],
        &home,
    );
    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    assert_eq!(
        stdout,
        format!(
            "➤ Uninstall\n  App          {}\n  ! Dry run. No files changed.\n",
            app.display()
        )
    );
    assert_eq!(install_mutations(&home), Vec::<String>::new());
}

#[test]
fn non_tty_app_install_requires_yes_and_still_prints_the_plan() {
    let home = isolated_home();
    let app = marker_app(&home);
    let (status, stdout, stderr) = run(&["install", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 1);
    assert_eq!(
        stderr,
        "non-interactive install requires --yes\n  incodex install --yes\n"
    );
    assert!(stdout.contains("➤ Install"));
    assert!(stdout.contains(&format!("  App          {}", app.display())));
    assert_eq!(install_mutations(&home), Vec::<String>::new());
    assert_eq!(
        fs::read_to_string(app.join("marker")).unwrap(),
        "do-not-touch\n"
    );
}

#[test]
fn clone_dry_run_does_not_create_scratch() {
    let home = isolated_home();
    let (status, stdout, stderr) = run(&["install", "--clone", "--dry-run"], &home);
    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    assert!(stdout.contains("➤ Clone install"));
    assert!(stdout.contains("  ! Dry run. No files changed."));
    assert!(!home.join(".incodex").join("scratch").exists());
}

#[test]
fn recover_missing_transaction_matches_golden() {
    let home = isolated_home();
    let (status, stdout, stderr) = run(&["recover", "--transaction", "does-not-exist"], &home);
    assert_eq!(status, 1);
    assert_eq!(stdout, "");
    assert_eq!(stderr, "no journal for does-not-exist\n");
}

#[test]
fn install_yes_app_patches_asar_writes_runtime_and_commits() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let official = PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/app.asar");
    let official_before = official
        .exists()
        .then(|| fs::read(&official).ok())
        .flatten();
    let cua_app = app.join("Contents/Frameworks/CUALockScreenGuardian.app");
    let cua = cua_app.join("Contents/MacOS/CUALockScreenGuardian");
    let cua_before = fs::read(&cua).unwrap();
    let cua_display_before = codesign_display(&cua_app);
    assert!(is_signed(&cua_app));

    let (status, stdout, stderr) =
        run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert_eq!(stderr, "");
    assert!(stdout.contains("➤ Install"));

    let asar = app.join("Contents/Resources/app.asar");
    let archive = Archive::open(&asar).unwrap();
    let pkg: serde_json::Value =
        serde_json::from_slice(&archive.extract("package.json").unwrap()).unwrap();
    assert_eq!(pkg["main"], LOADER_NAME);
    assert_eq!(pkg[MARKER_KEY]["originalMain"], "index.js");
    assert_eq!(
        String::from_utf8(archive.extract("index.js").unwrap()).unwrap(),
        "ok\n"
    );
    assert!(archive.extract(LOADER_NAME).is_ok());
    assert!(archive.has_only_loader());

    assert!(home.join(".incodex").join("runtime").exists());
    let journals: Vec<_> = install_mutations(&home)
        .into_iter()
        .filter(|path| path.ends_with("journal.json"))
        .collect();
    assert_eq!(journals.len(), 1);
    let journal: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(home.join(".incodex").join(&journals[0])).unwrap(),
    )
    .unwrap();
    assert_eq!(journal["phase"], "COMMITTED");
    assert_eq!(journal["schemaVersion"], 2);

    assert_eq!(fs::read(&cua).unwrap(), cua_before);
    assert!(
        is_signed(&cua_app),
        "vendor CUA sidecar signature must survive"
    );
    let cua_display = codesign_display(&cua_app);
    assert_eq!(
        cua_display, cua_display_before,
        "vendor CUA signature must be preserved"
    );
    assert!(
        !cua_display.to_lowercase().contains("2dc432gll2"),
        "{cua_display}"
    );

    if let Some(before) = official_before {
        assert_eq!(fs::read(&official).unwrap(), before);
    }
}

#[test]
fn install_codesign_failure_aborts_custom_target_before_swap() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let asar = app.join("Contents/Resources/app.asar");
    let before = fs::read(&asar).unwrap();
    let cua = app
        .join("Contents/Frameworks/CUALockScreenGuardian.app/Contents/MacOS/CUALockScreenGuardian");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    write_executable(
        &fake_bin.join("codesign"),
        "#!/bin/sh\nprintf '%s\\n' 'forced codesign failure' >&2\nexit 1\n",
    );

    let (status, _stdout, stderr) = run_with_path(
        &["install", "--yes", "--app", app.to_str().unwrap()],
        &home,
        &path_with_fake_bin(&fake_bin),
    );
    assert_eq!(status, 1, "stderr={stderr}");
    assert!(stderr.contains("forced codesign failure"), "{stderr}");
    assert_eq!(fs::read(&asar).unwrap(), before);
    assert!(
        cua.exists(),
        "vendor helper must be restored after sign failure"
    );
}

#[test]
fn install_aborts_when_asar_integrity_cannot_be_written() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let asar = app.join("Contents/Resources/app.asar");
    let before = fs::read(&asar).unwrap();
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    write_executable(&fake_bin.join("plutil"), "#!/bin/sh\nexit 1\n");
    write_executable(&fake_bin.join("codesign"), "#!/bin/sh\nexit 0\n");

    let (status, _stdout, stderr) = run_with_path(
        &["install", "--yes", "--app", app.to_str().unwrap()],
        &home,
        &path_with_fake_bin(&fake_bin),
    );
    assert_eq!(status, 1, "stderr={stderr}");
    assert!(
        stderr.contains("ElectronAsarIntegrity") || stderr.contains("plutil"),
        "{stderr}"
    );
    assert_eq!(fs::read(&asar).unwrap(), before);
}

#[test]
fn install_does_not_skip_a_stale_loader_without_a_committed_transaction() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let fake = home.join("fake-loader-only");
    fs::create_dir_all(&fake).unwrap();
    fs::write(
        fake.join("package.json"),
        format!(
            "{}\n",
            serde_json::json!({
                "main": LOADER_NAME,
                (MARKER_KEY): {
                    "originalMain": "index.js",
                    "installId": "00000000-0000-4000-8000-000000000000"
                }
            })
        ),
    )
    .unwrap();
    fs::write(fake.join(LOADER_NAME), "stale loader\n").unwrap();
    fs::write(fake.join("index.js"), "ok\n").unwrap();
    pack_dir(&fake, &app.join("Contents/Resources/app.asar")).unwrap();

    let (status, stdout, stderr) =
        run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert!(!stdout.contains("Already current"), "{stdout}");
    let archive = Archive::open(app.join("Contents/Resources/app.asar")).unwrap();
    assert_ne!(archive.extract(LOADER_NAME).unwrap(), b"stale loader\n");
}

#[test]
fn uninstall_refuses_while_another_command_holds_the_target_lock() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let (status, _, stderr) = run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0, "{stderr}");
    let asar = app.join("Contents/Resources/app.asar");
    let patched = fs::read(&asar).unwrap();
    let root = home.join(".incodex");
    let _lock = acquire_target_lock(&root, &app, "test-holder", None).unwrap();

    let (status, _stdout, stderr) = run(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &home,
    );
    assert_eq!(status, 1, "{stderr}");
    assert!(
        stderr.contains("another incodex command is modifying this app"),
        "{stderr}"
    );
    assert_eq!(fs::read(asar).unwrap(), patched);
}

#[test]
fn post_swap_verification_failure_rolls_back_the_original_app() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let asar = app.join("Contents/Resources/app.asar");
    let before = fs::read(&asar).unwrap();
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    write_executable(
        &fake_bin.join("codesign"),
        &format!(
            "#!/bin/sh\nlast=''\nfor arg in \"$@\"; do last=\"$arg\"; done\nif [ \"$1\" = \"--verify\" ] && [ \"$last\" = '{}' ]; then\n  printf '%s\\n' 'forced post-swap verification failure' >&2\n  exit 1\nfi\nexit 0\n",
            app.display()
        ),
    );

    let (status, _stdout, stderr) = run_with_path(
        &["install", "--yes", "--app", app.to_str().unwrap()],
        &home,
        &path_with_fake_bin(&fake_bin),
    );
    assert_eq!(status, 1, "{stderr}");
    assert!(
        stderr.contains("post-swap") || stderr.contains("verification"),
        "{stderr}"
    );
    assert_eq!(fs::read(&asar).unwrap(), before);
    let journal_path = install_mutations(&home)
        .into_iter()
        .find(|path| path.ends_with("journal.json"))
        .unwrap();
    let journal: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(home.join(".incodex").join(journal_path)).unwrap(),
    )
    .unwrap();
    assert_eq!(journal["phase"], "ROLLED_BACK");
}

#[test]
fn recover_does_not_finish_until_the_restored_app_verifies() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let root = home.join(".incodex");
    let (status, _, stderr) = run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0, "{stderr}");

    let staged = home.join("broken-staged.app");
    let copied = Command::new("ditto")
        .args([&app, &staged])
        .status()
        .unwrap();
    assert!(copied.success());
    fs::write(
        staged.join("Contents/MacOS/ChatGPT"),
        "broken after signing\n",
    )
    .unwrap();
    let mut tx = Engine::begin(&root, &app, "test-crash").unwrap();
    tx.place_staging(&staged).unwrap();
    tx.swap().unwrap();
    let id = tx.install_id().to_string();
    drop(tx);

    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    write_executable(&fake_bin.join("codesign"), "#!/bin/sh\nexit 1\n");
    let (status, _stdout, stderr) = run_with_path(
        &["recover", "--transaction", &id],
        &home,
        &path_with_fake_bin(&fake_bin),
    );
    assert_eq!(status, 1, "{stderr}");
    assert!(
        stderr.contains("restored target failed codesign verification"),
        "{stderr}"
    );
    let journal: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.join("transactions").join(id).join("journal.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(journal["phase"], "SWAPPED");
}

#[test]
fn uninstall_yes_app_restores_original_asar() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let (status, stdout, stderr) =
        run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");

    let (status, stdout, stderr) = run(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &home,
    );
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert_eq!(stderr, "");
    assert!(stdout.contains("➤ Uninstall"));

    let archive = Archive::open(app.join("Contents/Resources/app.asar")).unwrap();
    let pkg: serde_json::Value =
        serde_json::from_slice(&archive.extract("package.json").unwrap()).unwrap();
    assert_eq!(pkg["main"], "index.js");
    assert!(archive.extract(LOADER_NAME).is_err());
    assert_eq!(
        String::from_utf8(archive.extract("index.js").unwrap()).unwrap(),
        "ok\n"
    );
}

#[test]
fn recover_committed_transaction_is_done() {
    let home = isolated_home();
    let app = patchable_app(&home);
    let (status, _, stderr) = run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);
    assert_eq!(status, 0, "{stderr}");
    let journal_path = install_mutations(&home)
        .into_iter()
        .find(|path| path.ends_with("journal.json"))
        .expect("journal");
    let journal: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(home.join(".incodex").join(&journal_path)).unwrap(),
    )
    .unwrap();
    let id = journal["installId"].as_str().expect("installId");
    let asar_before = fs::read(app.join("Contents/Resources/app.asar")).unwrap();

    let (status, stdout, stderr) = run(&["recover", "--transaction", id], &home);
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert_eq!(stderr, "");
    assert_eq!(
        fs::read(app.join("Contents/Resources/app.asar")).unwrap(),
        asar_before
    );
}
