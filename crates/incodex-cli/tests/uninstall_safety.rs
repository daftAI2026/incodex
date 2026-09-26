use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_asar::{pack_dir, Archive, LOADER_NAME};
use incodex_macos::ditto;
use sha2::{Digest, Sha256};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn home() -> PathBuf {
    let sequence = SEQ.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "incodex-uninstall-safety-{}-{now}-{sequence}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn seed_session_db_canary(home: &Path) -> PathBuf {
    let sessions = home.join(".codex/sessions");
    fs::create_dir_all(&sessions).unwrap();
    let canary = sessions.join("synthetic-session-db.fixture");
    fs::write(&canary, b"synthetic Codex session DB canary v1\n").unwrap();
    canary
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn assert_session_db_canary_unchanged(path: &Path, before: &[u8], before_sha256: &str) {
    assert!(
        path.parent().is_some_and(Path::is_dir),
        "session DB directory disappeared"
    );
    let after = fs::read(path).expect("session DB canary was removed");
    assert_eq!(after, before, "session DB canary bytes changed");
    let after_sha256 = sha256_hex(&after);
    assert_eq!(
        after_sha256, before_sha256,
        "session DB canary hash changed"
    );
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

fn executable(path: &Path) {
    fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
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
<key>CFBundleIdentifier</key><string>com.example.incodex</string>
<key>CFBundleExecutable</key><string>ChatGPT</string>
</dict></plist>
"#,
    )
    .unwrap();
    executable(&contents.join("MacOS/ChatGPT"));
    let source = root.join("asar-src");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("package.json"), "{\"main\":\"index.js\"}\n").unwrap();
    fs::write(source.join("index.js"), "original\n").unwrap();
    pack_dir(&source, &contents.join("Resources/app.asar")).unwrap();
    app
}

fn install(root: &Path, app: &Path) -> String {
    let (status, stdout, stderr) = run(&["install", "--yes", "--app", app.to_str().unwrap()], root);
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    Archive::open(app.join("Contents/Resources/app.asar"))
        .unwrap()
        .read_package_main()
        .unwrap()
        .install_id
        .unwrap()
}

#[test]
fn uninstall_refuses_symlink_backup_and_foreign_live() {
    let root = home();
    let app = patchable_app(&root);
    let id = install(&root, &app);
    let original = root
        .join(".incodex/transactions")
        .join(&id)
        .join("original/ChatGPT.app");
    let victim = root.join("victim.app");
    ditto(&app, &victim).unwrap();
    let asar_before = fs::read(app.join("Contents/Resources/app.asar")).unwrap();
    fs::remove_dir_all(&original).unwrap();
    symlink(&victim, &original).unwrap();

    let (status, _stdout, stderr) = run(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &root,
    );
    assert_eq!(status, 1, "symlink backup was accepted: {stderr}");
    assert_eq!(
        fs::read(app.join("Contents/Resources/app.asar")).unwrap(),
        asar_before
    );
    assert!(victim.exists());

    let root = home();
    let app = patchable_app(&root);
    let _id = install(&root, &app);
    let foreign = root.join("foreign.app");
    ditto(&app, &foreign).unwrap();
    fs::remove_dir_all(&app).unwrap();
    fs::rename(&foreign, &app).unwrap();

    let (status, _stdout, stderr) = run(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &root,
    );
    assert_eq!(status, 1, "foreign live target was overwritten: {stderr}");
    assert!(
        Archive::open(app.join("Contents/Resources/app.asar"))
            .unwrap()
            .read_package_main()
            .unwrap()
            .already_patched
    );
    assert!(Archive::open(app.join("Contents/Resources/app.asar"))
        .unwrap()
        .extract(LOADER_NAME)
        .is_ok());
}

fn install_with_redirected_user_root() -> (PathBuf, PathBuf, PathBuf, String) {
    let sandbox = home();
    let requested_home = sandbox.join("requested-home");
    let backing_home = sandbox.join("backing-home");
    fs::create_dir_all(&requested_home).unwrap();
    fs::create_dir_all(&backing_home).unwrap();
    seed_session_db_canary(&requested_home);
    let app = patchable_app(&sandbox);
    let install_id = install(&backing_home, &app);
    let backing_root = backing_home.join(".incodex");
    fs::write(
        backing_root.join("outside-root-canary"),
        b"keep-external-data\n",
    )
    .unwrap();
    symlink(&backing_root, requested_home.join(".incodex")).unwrap();
    (sandbox, requested_home, backing_root, install_id)
}

#[test]
fn uninstall_refuses_a_symlinked_user_root_without_touching_its_target() {
    let (sandbox, requested_home, backing_root, install_id) = install_with_redirected_user_root();
    let app = sandbox.join("ChatGPT.app");
    let asar = app.join("Contents/Resources/app.asar");
    let before = fs::read(&asar).unwrap();
    let session_db = requested_home.join(".codex/sessions/synthetic-session-db.fixture");
    let session_db_before = fs::read(&session_db).unwrap();
    let session_db_sha256 = sha256_hex(&session_db_before);
    let transaction = backing_root.join("transactions").join(&install_id);
    let canary = backing_root.join("outside-root-canary");
    let canary_before = fs::read(&canary).unwrap();

    let (status, _stdout, stderr) = run(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &requested_home,
    );

    assert_session_db_canary_unchanged(&session_db, &session_db_before, &session_db_sha256);
    assert!(
        fs::read(&asar).unwrap() == before,
        "uninstall changed the app through a symlinked user root"
    );
    assert!(
        transaction.is_dir(),
        "transaction was removed through symlink"
    );
    assert_eq!(fs::read(&canary).unwrap(), canary_before);
    assert_eq!(status, 1, "symlinked user root was followed: {stderr}");
    assert!(stderr.contains("symlink"), "{stderr}");
}

#[test]
fn recover_refuses_a_symlinked_user_root_without_cleaning_its_target() {
    let (_sandbox, requested_home, backing_root, install_id) = install_with_redirected_user_root();
    let transaction = backing_root.join("transactions").join(&install_id);
    let canary = backing_root.join("outside-root-canary");
    let canary_before = fs::read(&canary).unwrap();
    let session_db = requested_home.join(".codex/sessions/synthetic-session-db.fixture");
    let session_db_before = fs::read(&session_db).unwrap();
    let session_db_sha256 = sha256_hex(&session_db_before);
    let journal = fs::read(transaction.join("journal.json")).unwrap();
    fs::write(
        backing_root
            .join("transactions")
            .join(format!(".cleanup-{install_id}.json")),
        journal,
    )
    .unwrap();

    let (status, _stdout, stderr) =
        run(&["recover", "--transaction", &install_id], &requested_home);

    assert_session_db_canary_unchanged(&session_db, &session_db_before, &session_db_sha256);
    assert!(
        transaction.is_dir(),
        "transaction was removed through symlink"
    );
    assert_eq!(fs::read(&canary).unwrap(), canary_before);
    assert_eq!(status, 1, "symlinked user root was followed: {stderr}");
    assert!(stderr.contains("symlink"), "{stderr}");
}

#[test]
fn install_and_uninstall_leave_the_original_session_db_canary_unchanged() {
    let root = home();
    let app = patchable_app(&root);
    let session_db = seed_session_db_canary(&root);
    let session_db_before = fs::read(&session_db).unwrap();
    let session_db_sha256 = sha256_hex(&session_db_before);

    let install_id = install(&root, &app);
    let transaction = root.join(".incodex/transactions").join(&install_id);
    assert!(
        transaction.is_dir(),
        "install transaction was not committed"
    );
    assert_session_db_canary_unchanged(&session_db, &session_db_before, &session_db_sha256);

    let (status, stdout, stderr) = run(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &root,
    );

    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    assert!(
        !transaction.exists(),
        "committed transaction was not cleaned"
    );
    assert_session_db_canary_unchanged(&session_db, &session_db_before, &session_db_sha256);
}
