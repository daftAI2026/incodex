use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_asar::{pack_dir, patch_asar, Archive};
use incodex_cli::legacy_proof::{
    prove_legacy_ts_v1, prove_legacy_ts_v1_with_boundaries, prove_legacy_ts_v1_with_checkpoint,
    verify_official_vendor_bundle,
};
use incodex_cli::legacy_typescript::load_legacy_ts_v1;
use incodex_core::{canonical_path, target_id};
use incodex_macos::{ditto, read_architecture};
use incodex_transaction::acquire_target_lock;
use serde_json::json;
use sha2::{Digest, Sha256};

static SEQ: AtomicU64 = AtomicU64::new(0);
const INSTALL_ID: &str = "11111111-1111-4111-8111-111111111111";
const OTHER_INSTALL_ID: &str = "22222222-2222-4222-8222-222222222222";

struct ProofFixture {
    root: PathBuf,
    app: PathBuf,
    original_app: PathBuf,
    manifest: PathBuf,
    original_asar: Vec<u8>,
    state: incodex_cli::legacy_typescript::LegacyStructuralState,
}

impl ProofFixture {
    fn create() -> Self {
        let root = temp_root();
        let app = root.join("apps/ChatGPT.app");
        let contents = app.join("Contents");
        fs::create_dir_all(contents.join("Resources")).unwrap();
        compile_executable(&contents.join("MacOS/ChatGPT"));
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.example.incodex-proof</string>
  <key>CFBundleShortVersionString</key><string>1.0.0</string>
  <key>CFBundleVersion</key><string>100</string>
  <key>CFBundleExecutable</key><string>ChatGPT</string>
</dict></plist>
"#;
        fs::write(contents.join("Info.plist"), plist).unwrap();

        let source = root.join("asar-source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("package.json"), "{\"main\":\"index.js\"}\n").unwrap();
        fs::write(source.join("index.js"), "original\n").unwrap();
        let asar = contents.join("Resources/app.asar");
        pack_dir(&source, &asar).unwrap();
        let original_asar = fs::read(&asar).unwrap();
        let original_archive = Archive::open(&asar).unwrap();
        let original_header_hash = original_archive.header_hash();
        let original_file_hash = original_archive.file_hash();
        sign_app(&app);

        let state_root = root.join(".incodex");
        let target = canonical_path(&app);
        let original_app = state_root
            .join("installations")
            .join(target_id(&app))
            .join(INSTALL_ID)
            .join("original/ChatGPT.app");
        ditto(&app, &original_app).unwrap();
        let (patched_header_hash, _) =
            patch_asar(&asar, "legacy-loader\n", Some(INSTALL_ID)).unwrap();
        let patched_archive = Archive::open(&asar).unwrap();
        let patched_file_hash = patched_archive.file_hash();
        sign_app(&app);

        let architecture = read_architecture(&app, "ChatGPT").unwrap();
        let target_store = state_root.join("installations").join(target_id(&app));
        let install_dir = target_store.join(INSTALL_ID);
        fs::create_dir_all(install_dir.join("patched")).unwrap();
        fs::write(
            target_store.join("current.json"),
            format!("{}\n", json!({"installId": INSTALL_ID})),
        )
        .unwrap();
        let manifest = install_dir.join("manifest.json");
        fs::write(
            &manifest,
            format!(
                "{}\n",
                json!({
                    "schemaVersion": 1,
                    "installId": INSTALL_ID,
                    "targetRealPath": target,
                    "bundleIdentifier": "com.example.incodex-proof",
                    "appVersion": "1.0.0",
                    "appBuild": "100",
                    "architecture": architecture,
                    "originalAsarHeaderHash": original_header_hash,
                    "originalAsarFileHash": original_file_hash,
                    "originalPlistFileHash": sha256(&fs::read(contents.join("Info.plist")).unwrap()),
                    "patchedAsarHeaderHash": patched_header_hash,
                    "patchedAsarFileHash": patched_file_hash,
                    "originalMain": "index.js",
                    "runtimeVersion": "0.2.0",
                    "createdAt": "2026-08-20T00:00:00.000Z",
                    "transactionState": "committed"
                })
            ),
        )
        .unwrap();
        fs::write(
            install_dir.join("patched/runtime-manifest.json"),
            format!(
                "{}\n",
                json!({
                    "installId": INSTALL_ID,
                    "originalMain": "index.js",
                    "patchedAsarHeaderHash": patched_header_hash,
                    "patchedAsarFileHash": patched_file_hash
                })
            ),
        )
        .unwrap();
        let transactions = state_root.join("transactions");
        fs::create_dir_all(&transactions).unwrap();
        fs::write(
            transactions.join(format!("{INSTALL_ID}.json")),
            format!(
                "{}\n",
                json!({
                    "schemaVersion": 1,
                    "installId": INSTALL_ID,
                    "targetRealPath": target,
                    "stagedApp": state_root.join(format!("scratch/ChatGPT.app.staged-{INSTALL_ID}")),
                    "originalSnapshot": original_app,
                    "outgoingApp": state_root.join(format!("transactions/{INSTALL_ID}/outgoing/ChatGPT.app")),
                    "phase": "COMMITTED",
                    "updatedAt": "2026-08-20T00:00:00.000Z"
                })
            ),
        )
        .unwrap();
        let state = load_legacy_ts_v1(&state_root, &app)
            .unwrap()
            .expect("committed TS fixture");
        Self {
            root: state_root,
            app,
            original_app,
            manifest,
            original_asar,
            state,
        }
    }

    fn live_asar(&self) -> PathBuf {
        self.app.join("Contents/Resources/app.asar")
    }

    fn original_asar_path(&self) -> PathBuf {
        self.original_app.join("Contents/Resources/app.asar")
    }
}

#[test]
fn committed_typescript_state_proves_live_and_backup_identity() {
    let fixture = ProofFixture::create();
    let proven = prove_legacy_ts_v1(&fixture.root, fixture.state).expect("proof gate");
    proven.with_locked(|structural, evidence| {
        assert_eq!(structural.install_id, INSTALL_ID);
        assert_eq!(evidence.live_install_id, INSTALL_ID);
        assert_eq!(
            evidence.original_asar_file_hash,
            sha256(&fixture.original_asar)
        );
    });
}

#[test]
fn proof_rejects_live_marker_hash_build_and_bundle_mismatches() {
    let fixture = ProofFixture::create();
    patch_asar(
        &fixture.live_asar(),
        "legacy-loader\n",
        Some(OTHER_INSTALL_ID),
    )
    .unwrap();
    sign_app(&fixture.app);
    let error = prove_legacy_ts_v1(&fixture.root, fixture.state).unwrap_err();
    assert!(error.contains("installId"), "{error}");

    let fixture = ProofFixture::create();
    fs::OpenOptions::new()
        .append(true)
        .open(fixture.live_asar())
        .unwrap()
        .write_all(b"tampered")
        .unwrap();
    let error = prove_legacy_ts_v1(&fixture.root, fixture.state).unwrap_err();
    assert!(error.contains("patched ASAR file hash"), "{error}");

    for field in ["appBuild", "bundleIdentifier"] {
        let fixture = ProofFixture::create();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&fixture.manifest).unwrap()).unwrap();
        manifest[field] = json!(if field == "appBuild" {
            "different"
        } else {
            "other.bundle"
        });
        fs::write(
            &fixture.manifest,
            format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap()),
        )
        .unwrap();
        let state = load_legacy_ts_v1(&fixture.root, &fixture.app)
            .unwrap()
            .expect("structural state after manifest edit");
        let error = prove_legacy_ts_v1(&fixture.root, state).unwrap_err();
        assert!(error.contains(field), "{field}: {error}");
    }
}

#[test]
fn proof_rejects_patched_or_changed_or_unsigned_backup() {
    let fixture = ProofFixture::create();
    patch_asar(
        &fixture.original_asar_path(),
        "legacy-loader\n",
        Some(INSTALL_ID),
    )
    .unwrap();
    let error = prove_legacy_ts_v1(&fixture.root, fixture.state).unwrap_err();
    assert!(error.contains("marker"), "{error}");

    let fixture = ProofFixture::create();
    fs::OpenOptions::new()
        .append(true)
        .open(fixture.original_asar_path())
        .unwrap()
        .write_all(b"tampered")
        .unwrap();
    let error = prove_legacy_ts_v1(&fixture.root, fixture.state).unwrap_err();
    assert!(error.contains("original ASAR file hash"), "{error}");

    let fixture = ProofFixture::create();
    Command::new("codesign")
        .args(["--remove-signature", "--"])
        .arg(&fixture.original_app)
        .status()
        .unwrap();
    let error = prove_legacy_ts_v1(&fixture.root, fixture.state).unwrap_err();
    assert!(error.contains("signature"), "{error}");
}

#[test]
fn proof_rejects_backup_inode_and_ancestor_replacement() {
    let fixture = ProofFixture::create();
    let old = fixture.original_app.with_file_name("ChatGPT.app.old");
    fs::rename(&fixture.original_app, &old).unwrap();
    ditto(&old, &fixture.original_app).unwrap();
    let error = prove_legacy_ts_v1(&fixture.root, fixture.state).unwrap_err();
    assert!(error.contains("identity"), "{error}");

    let fixture = ProofFixture::create();
    let parent = fixture.original_app.parent().unwrap().to_path_buf();
    let real = parent.with_file_name("original.real");
    fs::rename(&parent, &real).unwrap();
    std::os::unix::fs::symlink(&real, &parent).unwrap();
    let error = prove_legacy_ts_v1(&fixture.root, fixture.state).unwrap_err();
    assert!(error.to_lowercase().contains("symlink"), "{error}");
}

#[test]
fn proof_requires_the_target_lock_and_rechecks_toctou_identity() {
    let fixture = ProofFixture::create();
    let lock = acquire_target_lock(&fixture.root, &fixture.app, "test-holder", None).unwrap();
    let error = prove_legacy_ts_v1(&fixture.root, fixture.state).unwrap_err();
    assert!(error.contains("another incodex command"), "{error}");
    drop(lock);

    let fixture = ProofFixture::create();
    let old = fixture.app.with_file_name("ChatGPT.app.old");
    let app = fixture.app.clone();
    let error = prove_legacy_ts_v1_with_checkpoint(&fixture.root, fixture.state, move || {
        fs::rename(&app, &old).unwrap();
        fs::create_dir_all(&app).unwrap();
        Ok(())
    })
    .unwrap_err();
    assert!(
        error.contains("inode") || error.contains("real path"),
        "{error}"
    );
}

#[test]
fn proof_reopens_and_rehashes_same_inode_live_files_at_the_final_boundary() {
    let fixture = ProofFixture::create();
    let live_asar = fixture.live_asar();
    let live_plist = fixture.app.join("Contents/Info.plist");
    let error = prove_legacy_ts_v1_with_boundaries(
        &fixture.root,
        fixture.state,
        || Ok(()),
        move || {
            fs::OpenOptions::new()
                .append(true)
                .open(&live_asar)
                .unwrap()
                .write_all(b"same-inode-live-tamper")
                .unwrap();
            fs::OpenOptions::new()
                .append(true)
                .open(&live_plist)
                .unwrap()
                .write_all(b"same-inode-live-plist-tamper")
                .unwrap();
            Ok(())
        },
    )
    .unwrap_err();
    assert!(error.contains("final") || error.contains("hash"), "{error}");
}

#[test]
fn proof_reopens_and_rehashes_same_inode_backup_files_at_the_final_boundary() {
    let fixture = ProofFixture::create();
    let original_asar = fixture.original_asar_path();
    let original_plist = fixture.original_app.join("Contents/Info.plist");
    let error = prove_legacy_ts_v1_with_boundaries(
        &fixture.root,
        fixture.state,
        || Ok(()),
        move || {
            fs::OpenOptions::new()
                .append(true)
                .open(&original_asar)
                .unwrap()
                .write_all(b"same-inode-backup-tamper")
                .unwrap();
            fs::OpenOptions::new()
                .append(true)
                .open(&original_plist)
                .unwrap()
                .write_all(b"same-inode-backup-plist-tamper")
                .unwrap();
            Ok(())
        },
    )
    .unwrap_err();
    assert!(error.contains("final") || error.contains("hash"), "{error}");
}

#[test]
fn proof_rejects_live_internal_info_and_executable_symlinks() {
    for relative in ["Contents/Info.plist", "Contents/MacOS/ChatGPT"] {
        let fixture = ProofFixture::create();
        let path = fixture.app.join(relative);
        let victim = fixture
            .root
            .join(format!("victim-{}", relative.replace('/', "-")));
        fs::rename(&path, &victim).unwrap();
        std::os::unix::fs::symlink(&victim, &path).unwrap();
        let error = prove_legacy_ts_v1(&fixture.root, fixture.state).unwrap_err();
        assert!(
            error.to_lowercase().contains("symlink"),
            "{relative}: {error}"
        );
    }
}

#[test]
fn proof_rejects_backup_internal_info_symlink() {
    let fixture = ProofFixture::create();
    let path = fixture.original_app.join("Contents/Info.plist");
    let victim = fixture.root.join("backup-info-victim");
    fs::rename(&path, &victim).unwrap();
    std::os::unix::fs::symlink(&victim, &path).unwrap();
    let error = prove_legacy_ts_v1(&fixture.root, fixture.state).unwrap_err();
    assert!(error.to_lowercase().contains("symlink"), "{error}");
}

#[test]
fn ad_hoc_fixture_cannot_pass_the_official_vendor_verifier() {
    let fixture = ProofFixture::create();
    let error = verify_official_vendor_bundle(&fixture.original_app, "com.example.incodex-proof")
        .unwrap_err();
    assert!(
        error.to_lowercase().contains("ad hoc") || error.contains("vendor"),
        "{error}"
    );
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
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn sign_app(app: &Path) {
    assert!(Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-", "--"])
        .arg(app)
        .status()
        .unwrap()
        .success());
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn temp_root() -> PathBuf {
    let sequence = SEQ.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "incodex-legacy-proof-{}-{sequence}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}
