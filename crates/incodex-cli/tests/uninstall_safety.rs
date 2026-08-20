use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_asar::{pack_dir, Archive, LOADER_NAME};
use incodex_macos::ditto;

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
    assert_eq!(fs::read(app.join("Contents/Resources/app.asar")).unwrap(), asar_before);
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
