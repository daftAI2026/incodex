use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_asar::{pack_dir, patch_asar, Archive};
use incodex_core::target_id;
use serde_json::json;
use sha2::{Digest, Sha256};

static SEQ: AtomicU64 = AtomicU64::new(0);

const INSTALL_ID: &str = "11111111-1111-4111-8111-111111111111";
const INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.example.incodex</string>
  <key>CFBundleShortVersionString</key><string>1.0.0</string>
  <key>CFBundleVersion</key><string>100</string>
  <key>CFBundleExecutable</key><string>ChatGPT</string>
</dict></plist>
"#;

struct LegacyTsV1Fixture {
    root: PathBuf,
    app: PathBuf,
    original: Vec<u8>,
}

impl LegacyTsV1Fixture {
    fn create() -> Self {
        let root = temp_root();
        let app = root.join("ChatGPT.app");
        let asar = app.join("Contents/Resources/app.asar");
        fs::create_dir_all(asar.parent().unwrap()).unwrap();
        fs::write(app.join("Contents/Info.plist"), INFO_PLIST).unwrap();
        let source = root.join("asar-src");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("package.json"),
            r#"{"main":"index.js"}
"#,
        )
        .unwrap();
        fs::write(source.join("index.js"), "legacy-original\n").unwrap();
        pack_dir(&source, &asar).unwrap();
        let original = fs::read(&asar).unwrap();
        let original_archive = Archive::open(&asar).unwrap();
        let original_header_hash = original_archive.header_hash();
        let original_file_hash = original_archive.file_hash();
        let (patched_header_hash, _) =
            patch_asar(&asar, "legacy-loader\n", Some(INSTALL_ID)).unwrap();
        let patched_file_hash = Archive::open(&asar).unwrap().file_hash();

        let root_dir = root.join(".incodex");
        let target = fs::canonicalize(&app).unwrap();
        let target_store = root_dir.join("installations").join(target_id(&app));
        let install_dir = target_store.join(INSTALL_ID);
        let original_app = install_dir.join("original/ChatGPT.app");
        fs::create_dir_all(original_app.join("Contents/Resources")).unwrap();
        fs::copy(
            app.join("Contents/Info.plist"),
            original_app.join("Contents/Info.plist"),
        )
        .unwrap();
        fs::write(original_app.join("Contents/Resources/app.asar"), &original).unwrap();
        fs::create_dir_all(install_dir.join("patched")).unwrap();
        fs::write(
            target_store.join("current.json"),
            format!("{}\n", json!({"installId": INSTALL_ID})),
        )
        .unwrap();
        fs::write(
            install_dir.join("manifest.json"),
            format!(
                "{}\n",
                json!({
                    "schemaVersion": 1,
                    "installId": INSTALL_ID,
                    "targetRealPath": target,
                    "bundleIdentifier": "com.example.incodex",
                    "appVersion": "1.0.0",
                    "appBuild": "100",
                    "architecture": "arm64",
                    "originalAsarHeaderHash": original_header_hash,
                    "originalAsarFileHash": original_file_hash,
                    "originalPlistFileHash": sha256_hex(INFO_PLIST.as_bytes()),
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
        let transaction_dir = root_dir.join("transactions");
        fs::create_dir_all(&transaction_dir).unwrap();
        fs::write(
            transaction_dir.join(format!("{INSTALL_ID}.json")),
            format!(
                "{}\n",
                json!({
                    "schemaVersion": 1,
                    "installId": INSTALL_ID,
                    "targetRealPath": target,
                    "stagedApp": root_dir.join(format!("scratch/ChatGPT.app.staged-{INSTALL_ID}")).display().to_string(),
                    "originalSnapshot": original_app,
                    "outgoingApp": root_dir.join(format!("transactions/{INSTALL_ID}/outgoing/ChatGPT.app")).display().to_string(),
                    "phase": "COMMITTED",
                    "updatedAt": "2026-08-20T00:00:00.000Z"
                })
            ),
        )
        .unwrap();

        Self {
            root: root_dir,
            app,
            original,
        }
    }

    fn target_store(&self) -> PathBuf {
        self.root.join("installations").join(target_id(&self.app))
    }

    fn install_dir(&self) -> PathBuf {
        self.target_store().join(INSTALL_ID)
    }

    fn journal_path(&self) -> PathBuf {
        self.root
            .join("transactions")
            .join(format!("{INSTALL_ID}.json"))
    }
}

fn sha256_hex(body: &[u8]) -> String {
    Sha256::digest(body)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn replace_with_symlink(path: &std::path::Path) {
    let name = path.file_name().unwrap().to_string_lossy();
    let real = path.with_file_name(format!("{name}.real"));
    fs::rename(path, &real).unwrap();
    symlink(real, path).unwrap();
}

fn assert_rejects_symlink(mutator: impl FnOnce(&LegacyTsV1Fixture)) {
    let fixture = LegacyTsV1Fixture::create();
    mutator(&fixture);
    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect_err("legacy state symlink must be rejected");
    assert!(error.to_lowercase().contains("symlink"), "{error}");
}

fn set_journal_phase(fixture: &LegacyTsV1Fixture, phase: &str) {
    let mut raw: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.journal_path()).unwrap()).unwrap();
    raw["phase"] = json!(phase);
    fs::write(
        fixture.journal_path(),
        format!("{}\n", serde_json::to_string_pretty(&raw).unwrap()),
    )
    .unwrap();
}

fn set_journal_path(fixture: &LegacyTsV1Fixture, field: &str, path: &std::path::Path) {
    let mut raw: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.journal_path()).unwrap()).unwrap();
    raw[field] = json!(path.display().to_string());
    fs::write(
        fixture.journal_path(),
        format!("{}\n", serde_json::to_string_pretty(&raw).unwrap()),
    )
    .unwrap();
}

fn temp_root() -> PathBuf {
    let sequence = SEQ.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "incodex-legacy-ts-v1-{}-{now}-{sequence}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn legacy_typescript_fixture_reproduces_the_v1_disk_contract_without_running_ts_cli() {
    let fixture = LegacyTsV1Fixture::create();
    let state = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect("legacy TS v1 fixture should be readable")
        .expect("fixture should be detected");

    assert_eq!(state.install_id, INSTALL_ID);
    assert_eq!(state.manifest.schema_version, 1);
    assert_eq!(state.manifest.transaction_state, "committed");
    assert_eq!(
        state.manifest.original_plist_file_hash,
        sha256_hex(INFO_PLIST.as_bytes())
    );
    assert_eq!(state.journal.schema_version, 1);
    assert_eq!(state.journal.phase, "COMMITTED");
    assert_eq!(
        state.journal.original_snapshot,
        state.original_app.display().to_string()
    );
    assert_eq!(
        fs::read(state.original_app.join("Contents/Resources/app.asar")).unwrap(),
        fixture.original
    );
    assert_eq!(
        Archive::open(fixture.app.join("Contents/Resources/app.asar"))
            .unwrap()
            .read_package_main()
            .unwrap()
            .install_id
            .as_deref(),
        Some(INSTALL_ID)
    );
}

#[test]
fn legacy_typescript_fixture_rejects_leaf_and_ancestor_symlinks() {
    assert_rejects_symlink(|fixture| replace_with_symlink(&fixture.target_store()));
    assert_rejects_symlink(|fixture| replace_with_symlink(&fixture.root.join("installations")));
    assert_rejects_symlink(|fixture| {
        replace_with_symlink(&fixture.target_store().join("current.json"))
    });
    assert_rejects_symlink(|fixture| {
        replace_with_symlink(&fixture.install_dir().join("manifest.json"))
    });
    assert_rejects_symlink(|fixture| replace_with_symlink(&fixture.install_dir().join("patched")));
    assert_rejects_symlink(|fixture| {
        replace_with_symlink(&fixture.install_dir().join("patched/runtime-manifest.json"))
    });
    assert_rejects_symlink(|fixture| replace_with_symlink(&fixture.install_dir().join("original")));
    assert_rejects_symlink(|fixture| {
        replace_with_symlink(&fixture.install_dir().join("original/ChatGPT.app"))
    });
    assert_rejects_symlink(|fixture| replace_with_symlink(&fixture.root.join("transactions")));
    assert_rejects_symlink(|fixture| replace_with_symlink(&fixture.journal_path()));
}

#[test]
fn legacy_typescript_fixture_rejects_symlinked_transaction_path_fields() {
    assert_rejects_symlink(|fixture| {
        let scratch = fixture.root.join("scratch");
        let real = fixture.root.join("scratch.real");
        fs::create_dir_all(&real).unwrap();
        symlink(&real, &scratch).unwrap();
    });
    assert_rejects_symlink(|fixture| {
        let outgoing = fixture
            .root
            .join(format!("transactions/{INSTALL_ID}/outgoing"));
        let real = fixture.root.join("outgoing.real");
        fs::create_dir_all(&real).unwrap();
        fs::create_dir_all(outgoing.parent().unwrap()).unwrap();
        symlink(&real, &outgoing).unwrap();
    });
}

#[test]
fn legacy_typescript_fixture_rejects_a_dangling_target_store_symlink() {
    let fixture = LegacyTsV1Fixture::create();
    let target_store = fixture.target_store();
    fs::remove_dir_all(&target_store).unwrap();
    symlink(
        target_store.with_file_name("missing-target-store"),
        target_store,
    )
    .unwrap();

    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect_err("dangling target store must not look absent");
    assert!(error.to_lowercase().contains("symlink"), "{error}");
}

#[test]
fn legacy_typescript_fixture_rejects_an_empty_optional_outgoing_path() {
    let fixture = LegacyTsV1Fixture::create();
    let mut raw: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.journal_path()).unwrap()).unwrap();
    raw["outgoingApp"] = json!("");
    fs::write(
        fixture.journal_path(),
        format!("{}\n", serde_json::to_string_pretty(&raw).unwrap()),
    )
    .unwrap();

    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect_err("empty outgoingApp must be rejected");
    assert!(error.contains("outgoingApp"), "{error}");
}

#[test]
fn legacy_typescript_fixture_accepts_only_emitted_staging_layouts() {
    let fixture = LegacyTsV1Fixture::create();
    set_journal_path(
        &fixture,
        "stagedApp",
        &fixture.root.join("ChatGPT.app.live"),
    );
    incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect("official live staging layout should be structurally valid")
        .expect("fixture should be detected");

    let fixture = LegacyTsV1Fixture::create();
    set_journal_path(&fixture, "stagedApp", &fixture.root);
    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect_err("the state root itself is not a staging layout");
    assert!(error.contains("stagedApp"), "{error}");

    let fixture = LegacyTsV1Fixture::create();
    set_journal_path(
        &fixture,
        "stagedApp",
        &fixture.root.join("scratch/other-staged-app"),
    );
    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect_err("an unrelated staging path must be rejected");
    assert!(error.contains("stagedApp"), "{error}");
}

#[test]
fn legacy_typescript_fixture_accepts_only_the_install_transaction_outgoing_layout() {
    let fixture = LegacyTsV1Fixture::create();
    set_journal_path(&fixture, "outgoingApp", &fixture.root);
    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect_err("the state root itself is not an outgoing app layout");
    assert!(error.contains("outgoingApp"), "{error}");

    let fixture = LegacyTsV1Fixture::create();
    set_journal_path(
        &fixture,
        "outgoingApp",
        &fixture
            .root
            .join(format!("transactions/{INSTALL_ID}/outgoing/Other.app")),
    );
    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect_err("an unrelated outgoing path must be rejected");
    assert!(error.contains("outgoingApp"), "{error}");

    let fixture = LegacyTsV1Fixture::create();
    set_journal_path(
        &fixture,
        "outgoingApp",
        &fixture
            .root
            .join("transactions/22222222-2222-4222-8222-222222222222/outgoing/ChatGPT.app"),
    );
    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect_err("an outgoing path for another install must be rejected");
    assert!(error.contains("outgoingApp"), "{error}");
}

#[test]
fn legacy_typescript_fixture_classifies_journal_phase_without_mixing_states() {
    for (phase, expected) in [
        (
            "COMMITTED",
            incodex_cli::legacy_typescript::LegacyStateKind::Committed,
        ),
        (
            "TARGET_VERIFIED",
            incodex_cli::legacy_typescript::LegacyStateKind::Interrupted,
        ),
        (
            "PATCHED",
            incodex_cli::legacy_typescript::LegacyStateKind::Interrupted,
        ),
        (
            "ROLLED_BACK",
            incodex_cli::legacy_typescript::LegacyStateKind::RolledBack,
        ),
    ] {
        let fixture = LegacyTsV1Fixture::create();
        set_journal_phase(&fixture, phase);
        let state = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
            .expect("phase should be structurally readable")
            .expect("fixture should be detected");
        assert_eq!(state.kind, expected, "phase {phase} was misclassified");
    }
}

#[test]
fn legacy_typescript_fixture_rejects_a_manifest_target_mismatch() {
    let fixture = LegacyTsV1Fixture::create();
    let manifest = fixture
        .root
        .join("installations")
        .join(target_id(&fixture.app))
        .join(INSTALL_ID)
        .join("manifest.json");
    let mut raw: serde_json::Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    raw["targetRealPath"] = json!(fixture.root.join("Other.app"));
    fs::write(
        &manifest,
        format!("{}\n", serde_json::to_string_pretty(&raw).unwrap()),
    )
    .unwrap();

    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect_err("mismatched target must not be accepted");
    assert!(error.contains("targetRealPath"), "{error}");
}
