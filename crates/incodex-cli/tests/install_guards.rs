use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use incodex_asar::{pack_dir, Archive, LOADER_NAME, MARKER_KEY};
use incodex_macos::sign_app;
use incodex_transaction::restore_committed;

static SEQ: AtomicU64 = AtomicU64::new(0);
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn serialize_signing() -> MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn home() -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("incodex-install-guard-{}-{n}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn run(args: &[&str], home: &Path) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(args)
        .env("HOME", home)
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .env("SHELL", "/bin/zsh")
        .output()
        .unwrap();
    (
        output.status.code().unwrap_or(1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn patchable_app(root: &Path) -> PathBuf {
    let app = root.join("ChatGPT.app");
    let contents = app.join("Contents");
    fs::create_dir_all(contents.join("Resources")).unwrap();
    fs::create_dir_all(contents.join("MacOS")).unwrap();
    fs::write(
        contents.join("Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.example.install-guard</string>
<key>CFBundleShortVersionString</key><string>1.0.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleExecutable</key><string>ChatGPT</string>
</dict></plist>
"#,
    )
    .unwrap();
    let executable = contents.join("MacOS/ChatGPT");
    fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let source = root.join("asar-source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("package.json"), "{\"main\":\"index.js\"}\n").unwrap();
    fs::write(source.join("index.js"), "original\n").unwrap();
    pack_dir(&source, &contents.join("Resources/app.asar")).unwrap();
    app
}

fn install(home: &Path, app: &Path) -> String {
    sign_app(app).unwrap();
    let (status, stdout, stderr) = run(&["install", "--yes", "--app", app.to_str().unwrap()], home);
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    Archive::open(app.join("Contents/Resources/app.asar"))
        .unwrap()
        .read_package_main()
        .unwrap()
        .install_id
        .unwrap()
}

fn transaction_ids(home: &Path) -> Vec<String> {
    let dir = home.join(".incodex/transactions");
    let mut ids = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn replace_with_unbound_marker(home: &Path, app: &Path) {
    let source = home.join("unbound-patched");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("package.json"),
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
    fs::write(source.join(LOADER_NAME), "unbound loader\n").unwrap();
    fs::write(source.join("index.js"), "already patched\n").unwrap();
    pack_dir(&source, &app.join("Contents/Resources/app.asar")).unwrap();
}

fn replace_with_unbound_loader(home: &Path, app: &Path) {
    let source = home.join("unbound-loader");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("package.json"), "{\"main\":\"index.js\"}\n").unwrap();
    fs::write(source.join(LOADER_NAME), "unbound loader\n").unwrap();
    fs::write(source.join("index.js"), "already modified\n").unwrap();
    pack_dir(&source, &app.join("Contents/Resources/app.asar")).unwrap();
}

#[test]
fn install_refuses_marked_live_app_without_trusted_record_before_snapshot() {
    let _guard = serialize_signing();
    let home = home();
    let app = patchable_app(&home);
    replace_with_unbound_marker(&home, &app);
    let asar = app.join("Contents/Resources/app.asar");
    let before = fs::read(&asar).unwrap();

    let (status, _stdout, stderr) =
        run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);

    assert_eq!(status, 1, "unbound patched app was accepted: {stderr}");
    assert!(
        stderr.contains("marker"),
        "refusal should name the marker: {stderr}"
    );
    assert_eq!(fs::read(&asar).unwrap(), before);
    assert!(transaction_ids(&home).is_empty());
}

#[test]
fn install_refuses_unbound_loader_without_marker_before_snapshot() {
    let _guard = serialize_signing();
    let home = home();
    let app = patchable_app(&home);
    replace_with_unbound_loader(&home, &app);
    let asar = app.join("Contents/Resources/app.asar");
    let before = fs::read(&asar).unwrap();

    let (status, _stdout, stderr) =
        run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);

    assert_eq!(status, 1, "unbound loader was accepted: {stderr}");
    assert!(
        stderr.contains("loader"),
        "refusal should name the loader: {stderr}"
    );
    assert_eq!(fs::read(&asar).unwrap(), before);
    assert!(transaction_ids(&home).is_empty());
}

#[test]
fn install_refuses_trusted_record_with_tampered_live_tree() {
    let _guard = serialize_signing();
    let home = home();
    let app = patchable_app(&home);
    let install_id = install(&home, &app);
    let ids_before = transaction_ids(&home);
    fs::write(app.join("Contents/live-tamper"), "must remain\n").unwrap();

    let (status, _stdout, stderr) =
        run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);

    assert_eq!(status, 1, "tampered live target was accepted: {stderr}");
    assert!(
        stderr.contains("marker"),
        "refusal should name the marker: {stderr}"
    );
    assert_eq!(transaction_ids(&home), ids_before);
    assert!(home
        .join(".incodex/transactions")
        .join(install_id)
        .join("original/ChatGPT.app")
        .exists());
}

#[test]
fn install_refuses_trusted_record_with_tampered_original_backup() {
    let _guard = serialize_signing();
    let home = home();
    let app = patchable_app(&home);
    let install_id = install(&home, &app);
    let ids_before = transaction_ids(&home);
    let original = home
        .join(".incodex/transactions")
        .join(install_id)
        .join("original/ChatGPT.app");
    fs::write(original.join("Contents/backup-tamper"), "must remain\n").unwrap();

    let (status, _stdout, stderr) =
        run(&["install", "--yes", "--app", app.to_str().unwrap()], &home);

    assert_eq!(status, 1, "tampered original backup was accepted: {stderr}");
    assert!(
        stderr.contains("marker"),
        "refusal should name the marker: {stderr}"
    );
    assert_eq!(transaction_ids(&home), ids_before);
    assert!(original.join("Contents/backup-tamper").exists());
}

#[test]
fn uninstall_removes_the_restored_transaction_backup() {
    let _guard = serialize_signing();
    let home = home();
    let app = patchable_app(&home);
    let install_id = install(&home, &app);
    let transaction = home.join(".incodex/transactions").join(&install_id);

    let (status, stdout, stderr) = run(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &home,
    );

    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert!(
        !transaction.exists(),
        "successful uninstall retained transaction {install_id}"
    );
}

#[test]
fn a_new_install_removes_the_superseded_committed_backup() {
    let _guard = serialize_signing();
    let home = home();
    let app = patchable_app(&home);
    let first_install_id = install(&home, &app);

    fs::remove_dir_all(&app).unwrap();
    let replacement = patchable_app(&home);
    let second_install_id = install(&home, &replacement);

    assert_ne!(first_install_id, second_install_id);
    assert_eq!(transaction_ids(&home), vec![second_install_id]);
}

#[test]
fn a_new_install_removes_a_superseded_restored_backup() {
    let _guard = serialize_signing();
    let home = home();
    let app = patchable_app(&home);
    let first_install_id = install(&home, &app);
    let root = home.join(".incodex");
    restore_committed(&root, &first_install_id, &app).unwrap();

    let second_install_id = install(&home, &app);

    assert_ne!(first_install_id, second_install_id);
    assert_eq!(transaction_ids(&home), vec![second_install_id]);
}

#[test]
fn recover_removes_a_restored_terminal_transaction() {
    let _guard = serialize_signing();
    let home = home();
    let app = patchable_app(&home);
    let install_id = install(&home, &app);
    let root = home.join(".incodex");
    let transaction = root.join("transactions").join(&install_id);
    restore_committed(&root, &install_id, &app).unwrap();

    let (status, stdout, stderr) = run(&["recover", "--transaction", install_id.as_str()], &home);

    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert!(
        !transaction.exists(),
        "recover retained restored transaction {install_id}"
    );
}
