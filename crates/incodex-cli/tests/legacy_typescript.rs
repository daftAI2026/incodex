use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_asar::{pack_dir, patch_asar, Archive};
use incodex_core::target_id;
use serde_json::json;

static SEQ: AtomicU64 = AtomicU64::new(0);

const INSTALL_ID: &str = "11111111-1111-4111-8111-111111111111";

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
                    "originalPlistFileHash": "plist-hash",
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
                    "stagedApp": root_dir.join("scratch/ChatGPT.app.staged").display().to_string(),
                    "originalSnapshot": original_app,
                    "outgoingApp": root_dir.join("transactions/outgoing/ChatGPT.app").display().to_string(),
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
