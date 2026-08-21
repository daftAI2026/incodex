use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_asar::{pack_dir, patch_asar, Archive};
use incodex_core::{canonical_path, target_id};
use incodex_macos::{ditto, read_architecture, sign_app, write_asar_integrity};
use serde_json::json;
use sha2::{Digest, Sha256};

static SEQ: AtomicU64 = AtomicU64::new(0);
pub const INSTALL_ID: &str = "33333333-3333-4333-8333-333333333333";

pub struct Fixture {
    pub root: PathBuf,
    pub app: PathBuf,
    pub original_app: PathBuf,
    pub original_bytes: Vec<u8>,
}

impl Fixture {
    pub fn create() -> Self {
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
        // Match the retired TS writer: patching the ASAR also rewrites the
        // live Info.plist ElectronAsarIntegrity entry before signing.
        write_asar_integrity(&app, &patched_file).unwrap();
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

    pub fn app_asar(&self) -> PathBuf {
        self.app.join("Contents/Resources/app.asar")
    }
    pub fn legacy_journal(&self) -> PathBuf {
        self.root
            .join("transactions")
            .join(format!("{INSTALL_ID}.json"))
    }
    pub fn set_phase(&self, phase: &str) {
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
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}
