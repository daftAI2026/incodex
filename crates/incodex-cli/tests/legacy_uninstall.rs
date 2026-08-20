use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_asar::{pack_dir, patch_asar, Archive, LOADER_NAME};
use incodex_macos::{sign_app, write_asar_integrity};
use incodex_transaction::journal_v2;
use serde::Serialize;
use sha2::{Digest, Sha256};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn home() -> PathBuf {
    let sequence = SEQ.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "incodex-legacy-uninstall-{}-{now}-{sequence}",
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
    let executable = contents.join("MacOS/ChatGPT");
    fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(executable, permissions).unwrap();
    let source = root.join("asar-src");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("package.json"), "{\"main\":\"index.js\"}\n").unwrap();
    fs::write(source.join("index.js"), "original\n").unwrap();
    pack_dir(&source, &contents.join("Resources/app.asar")).unwrap();
    sign_app(&app).unwrap();
    app
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyTarget {
    requested_path: String,
    real_path: String,
    device: String,
    inode: String,
}

#[derive(Serialize)]
struct LegacyPaths {
    staged: String,
    outgoing: String,
    original: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyJournal {
    schema_version: u32,
    install_id: String,
    target: LegacyTarget,
    paths: LegacyPaths,
    phase: String,
    sequence: u64,
    checksum: String,
}

fn rewrite_as_legacy(path: &Path) -> String {
    let raw: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    let target = &raw["target"];
    let mut legacy = LegacyJournal {
        schema_version: 2,
        install_id: raw["installId"].as_str().unwrap().to_string(),
        target: LegacyTarget {
            requested_path: target["requestedPath"].as_str().unwrap().to_string(),
            real_path: target["realPath"].as_str().unwrap().to_string(),
            device: target["device"].as_str().unwrap().to_string(),
            inode: target["inode"].as_str().unwrap().to_string(),
        },
        paths: LegacyPaths {
            staged: "staging/ChatGPT.app".into(),
            outgoing: "outgoing/ChatGPT.app".into(),
            original: "original/ChatGPT.app".into(),
        },
        phase: "COMMITTED".into(),
        sequence: raw["sequence"].as_u64().unwrap(),
        checksum: String::new(),
    };
    legacy.checksum = Sha256::digest(serde_json::to_vec(&legacy).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{}\n", serde_json::to_string_pretty(&legacy).unwrap())
}

#[test]
fn uninstall_migrates_legacy_committed_journal_before_restore() {
    let root = home();
    let app = patchable_app(&root);
    let (status, stdout, stderr) =
        run(&["install", "--yes", "--app", app.to_str().unwrap()], &root);
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    let archive = Archive::open(app.join("Contents/Resources/app.asar")).unwrap();
    let install_id = archive.read_package_main().unwrap().install_id.unwrap();
    let asar = app.join("Contents/Resources/app.asar");
    let legacy_loader = "module.exports = require('./index.js');\n";
    let (legacy_hash, _) = patch_asar(&asar, legacy_loader, Some(&install_id)).unwrap();
    write_asar_integrity(&app, &legacy_hash).unwrap();
    sign_app(&app).unwrap();
    let legacy = Archive::open(&asar).unwrap();
    assert_eq!(
        legacy.extract(LOADER_NAME).unwrap(),
        legacy_loader.as_bytes()
    );
    let journal_path = root
        .join(".incodex/transactions")
        .join(&install_id)
        .join("journal.json");
    let legacy = rewrite_as_legacy(&journal_path);
    fs::write(&journal_path, legacy).unwrap();

    let (status, stdout, stderr) = run(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &root,
    );
    assert_eq!(
        status, 0,
        "legacy uninstall failed: stdout={stdout}\nstderr={stderr}"
    );
    let restored = Archive::open(app.join("Contents/Resources/app.asar")).unwrap();
    assert_eq!(restored.read_package_main().unwrap().main, "index.js");
    assert!(restored.extract(LOADER_NAME).is_err());
    assert_eq!(
        journal_v2(&root.join(".incodex"), &install_id)
            .unwrap()
            .phase,
        "ROLLED_BACK"
    );
}

#[test]
fn legacy_uninstall_refuses_a_truncated_unsigned_backup() {
    let root = home();
    let app = patchable_app(&root);
    let (status, stdout, stderr) =
        run(&["install", "--yes", "--app", app.to_str().unwrap()], &root);
    assert_eq!(status, 0, "stdout={stdout}\nstderr={stderr}");
    let install_id = Archive::open(app.join("Contents/Resources/app.asar"))
        .unwrap()
        .read_package_main()
        .unwrap()
        .install_id
        .unwrap();
    let journal_path = root
        .join(".incodex/transactions")
        .join(&install_id)
        .join("journal.json");
    fs::write(&journal_path, rewrite_as_legacy(&journal_path)).unwrap();
    let original = root
        .join(".incodex/transactions")
        .join(&install_id)
        .join("original/ChatGPT.app");
    fs::remove_file(original.join("Contents/Resources/app.asar")).unwrap();
    let live_before = fs::read(app.join("Contents/Resources/app.asar")).unwrap();

    let (status, _stdout, stderr) = run(
        &["uninstall", "--yes", "--app", app.to_str().unwrap()],
        &root,
    );
    assert_eq!(status, 1, "invalid backup was restored: {stderr}");
    assert_eq!(
        fs::read(app.join("Contents/Resources/app.asar")).unwrap(),
        live_before
    );
}
