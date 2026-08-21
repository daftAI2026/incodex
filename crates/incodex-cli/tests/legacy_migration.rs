use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_asar::{pack_dir, patch_asar, Archive, LOADER_NAME};
use incodex_cli::legacy_migration::migrate_legacy_ts_v1;
use incodex_cli::legacy_proof::prove_legacy_ts_v1;
use incodex_cli::legacy_typescript::load_legacy_ts_v1;
use incodex_core::{canonical_path, target_id};
use incodex_macos::{ditto, read_architecture, sign_app};
use incodex_transaction::journal_v2;
use serde_json::json;
use sha2::{Digest, Sha256};

static SEQ: AtomicU64 = AtomicU64::new(0);
const INSTALL_ID: &str = "33333333-3333-4333-8333-333333333333";

struct Fixture {
    root: PathBuf,
    app: PathBuf,
    original_app: PathBuf,
    original_bytes: Vec<u8>,
}

impl Fixture {
    fn create() -> Self {
        let root = temp_root();
        let app = root.join("apps/ChatGPT.app");
        let contents = app.join("Contents");
        fs::create_dir_all(contents.join("Resources")).unwrap();
        compile_executable(&contents.join("MacOS/ChatGPT"));
        fs::write(
            contents.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.example.incodex-migration</string>
<key>CFBundleShortVersionString</key><string>1.0.0</string>
<key>CFBundleVersion</key><string>100</string>
<key>CFBundleExecutable</key><string>ChatGPT</string>
</dict></plist>
"#,
        )
        .unwrap();
        let source = root.join("asar-source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("package.json"), "{\"main\":\"index.js\"}\n").unwrap();
        fs::write(source.join("index.js"), "original\n").unwrap();
        let asar = contents.join("Resources/app.asar");
        pack_dir(&source, &asar).unwrap();
        let original_bytes = fs::read(&asar).unwrap();
        let original_archive = Archive::open(&asar).unwrap();
        let original_plist = sha256(&fs::read(contents.join("Info.plist")).unwrap());
        let original_header = original_archive.header_hash();
        let original_file = original_archive.file_hash();
        sign_app(&app).unwrap();

        let state_root = root.join(".incodex");
        let target = canonical_path(&app);
        let target_key = target_id(&app);
        let install_dir = state_root
            .join("installations")
            .join(&target_key)
            .join(INSTALL_ID);
        let original_app = install_dir.join("original/ChatGPT.app");
        ditto(&app, &original_app).unwrap();
        let (patched_header, _) = patch_asar(&asar, "legacy-loader\n", Some(INSTALL_ID)).unwrap();
        let patched_file = Archive::open(&asar).unwrap().file_hash();
        sign_app(&app).unwrap();
        let architecture = read_architecture(&app, "ChatGPT").unwrap();
        fs::create_dir_all(install_dir.join("patched")).unwrap();
        fs::write(
            state_root
                .join("installations")
                .join(&target_key)
                .join("current.json"),
            format!("{}\n", json!({"installId": INSTALL_ID})),
        )
        .unwrap();
        fs::write(
            install_dir.join("manifest.json"),
            format!(
                "{}\n",
                json!({
                    "schemaVersion": 1, "installId": INSTALL_ID,
                    "targetRealPath": target, "bundleIdentifier": "com.example.incodex-migration",
                    "appVersion": "1.0.0", "appBuild": "100", "architecture": architecture,
                    "originalAsarHeaderHash": original_header, "originalAsarFileHash": original_file,
                    "originalPlistFileHash": original_plist, "patchedAsarHeaderHash": patched_header,
                    "patchedAsarFileHash": patched_file, "originalMain": "index.js",
                    "runtimeVersion": "0.2.0", "createdAt": "2026-08-20T00:00:00.000Z",
                    "transactionState": "committed"
                })
            ),
        )
        .unwrap();
        fs::write(
            install_dir.join("patched/runtime-manifest.json"),
            format!(
                "{}\n",
                json!({"installId": INSTALL_ID, "originalMain": "index.js",
                    "patchedAsarHeaderHash": patched_header, "patchedAsarFileHash": patched_file})
            ),
        )
        .unwrap();
        let transactions = state_root.join("transactions");
        fs::create_dir_all(&transactions).unwrap();
        fs::write(
            transactions.join(format!("{INSTALL_ID}.json")),
            format!(
                "{}\n",
                json!({"schemaVersion": 1, "installId": INSTALL_ID, "targetRealPath": target,
                    "stagedApp": state_root.join(format!("scratch/ChatGPT.app.staged-{INSTALL_ID}")),
                    "originalSnapshot": original_app, "outgoingApp": state_root.join(format!("transactions/{INSTALL_ID}/outgoing/ChatGPT.app")),
                    "phase": "COMMITTED", "updatedAt": "2026-08-20T00:00:00.000Z"})
            ),
        )
        .unwrap();
        Self {
            root: state_root,
            app,
            original_app,
            original_bytes,
        }
    }

    fn app_asar(&self) -> PathBuf {
        self.app.join("Contents/Resources/app.asar")
    }
    fn legacy_journal(&self) -> PathBuf {
        self.root
            .join("transactions")
            .join(format!("{INSTALL_ID}.json"))
    }
    fn set_phase(&self, phase: &str) {
        let path = self.legacy_journal();
        let mut journal: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        journal["phase"] = json!(phase);
        fs::write(
            path,
            format!("{}\n", serde_json::to_string_pretty(&journal).unwrap()),
        )
        .unwrap();
    }
}

#[test]
fn committed_ts_state_migrates_to_v2_without_using_live_as_original() {
    let fixture = Fixture::create();
    let state = load_legacy_ts_v1(&fixture.root, &fixture.app)
        .unwrap()
        .unwrap();
    let proven = prove_legacy_ts_v1(&fixture.root, state).unwrap();
    let journal = migrate_legacy_ts_v1(&fixture.root, proven).unwrap();
    assert_eq!(journal.phase, "COMMITTED");
    assert_eq!(
        fs::read(fixture.original_app.join("Contents/Resources/app.asar")).unwrap(),
        fixture.original_bytes
    );
    assert_ne!(
        fs::read(fixture.app_asar()).unwrap(),
        fixture.original_bytes
    );
    assert!(fixture.legacy_journal().is_file());
    assert_eq!(
        journal_v2(&fixture.root, INSTALL_ID).unwrap().phase,
        "COMMITTED"
    );
}

#[test]
fn migration_round_trip_restores_exact_original_and_keeps_legacy_record() {
    let fixture = Fixture::create();
    let install = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["install", "--yes", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["uninstall", "--yes", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(fixture.app_asar()).unwrap(),
        fixture.original_bytes
    );
    assert!(Archive::open(fixture.app_asar())
        .unwrap()
        .extract(LOADER_NAME)
        .is_err());
    assert!(fixture.legacy_journal().is_file());
    assert_eq!(
        journal_v2(&fixture.root, INSTALL_ID).unwrap().phase,
        "ROLLED_BACK"
    );
}

#[test]
fn rust_install_adopts_legacy_state_without_using_patched_live_as_original() {
    let fixture = Fixture::create();
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["install", "--yes", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        journal_v2(&fixture.root, INSTALL_ID).unwrap().phase,
        "COMMITTED"
    );
    assert_ne!(
        fs::read(fixture.app_asar()).unwrap(),
        fixture.original_bytes
    );
    assert_eq!(
        fs::read(
            fixture
                .root
                .join("transactions")
                .join(INSTALL_ID)
                .join("original/ChatGPT.app/Contents/Resources/app.asar")
        )
        .unwrap(),
        fixture.original_bytes
    );
}

#[test]
fn rust_install_refuses_a_modified_legacy_backup_before_writing_v2() {
    let fixture = Fixture::create();
    let backup_asar = fixture
        .original_app
        .join("Contents/Resources/app.asar");
    fs::OpenOptions::new()
        .append(true)
        .open(backup_asar)
        .unwrap()
        .write_all(b"tampered")
        .unwrap();
    let live_before = fs::read(fixture.app_asar()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["install", "--yes", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(fixture.app_asar()).unwrap(), live_before);
    assert!(journal_v2(&fixture.root, INSTALL_ID).is_err());
}

#[test]
fn rust_install_refuses_a_live_marker_mismatch_before_writing_v2() {
    let fixture = Fixture::create();
    patch_asar(
        &fixture.app_asar(),
        "legacy-loader\n",
        Some("44444444-4444-4444-8444-444444444444"),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["install", "--yes", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(journal_v2(&fixture.root, INSTALL_ID).is_err());
}

#[test]
fn each_interrupted_ts_phase_recovers_without_silently_ignoring_the_flat_journal() {
    for phase in [
        "DISCOVERED",
        "BACKUP_COMMITTED",
        "STAGED",
        "PATCHED",
        "SIGNED",
        "VERIFIED",
        "TARGET_MOVED_OUT",
        "SWAPPED",
        "TARGET_VERIFIED",
    ] {
        let fixture = Fixture::create();
        fixture.set_phase(phase);
        if phase == "DISCOVERED" {
            let path = fixture.legacy_journal();
            let mut journal: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            journal.as_object_mut().unwrap().remove("outgoingApp");
            fs::write(
                path,
                format!("{}\n", serde_json::to_string_pretty(&journal).unwrap()),
            )
            .unwrap();
        }
        let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
            .args(["recover", "--transaction", INSTALL_ID])
            .env("HOME", fixture.root.parent().unwrap())
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "phase={phase}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let journal: serde_json::Value =
            serde_json::from_slice(&fs::read(fixture.legacy_journal()).unwrap()).unwrap();
        assert_eq!(journal["phase"], "ROLLED_BACK", "phase={phase}");
    }
}

#[test]
fn status_reports_a_committed_legacy_install_instead_of_a_missing_backup() {
    let fixture = Fixture::create();
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .args(["status", "--json", "--app", fixture.app.to_str().unwrap()])
        .env("HOME", fixture.root.parent().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["backup"]["legacy"], true);
    assert_eq!(report["backup"]["installId"], INSTALL_ID);
}

fn compile_executable(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let source = path.with_extension("c");
    fs::write(&source, "int main(void) { return 0; }\n").unwrap();
    assert!(Command::new("cc")
        .args(["-x", "c"])
        .arg(&source)
        .arg("-o")
        .arg(path)
        .status()
        .unwrap()
        .success());
    let _ = fs::remove_file(source);
    let mut mode = fs::metadata(path).unwrap().permissions();
    mode.set_mode(0o755);
    fs::set_permissions(path, mode).unwrap();
}
fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn temp_root() -> PathBuf {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "incodex-legacy-migration-{}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}
