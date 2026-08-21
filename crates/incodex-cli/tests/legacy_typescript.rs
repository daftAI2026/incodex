use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_asar::{pack_dir, patch_asar, Archive};
use incodex_core::{canonical_path, is_official_app, target_id};
use serde_json::json;
use sha2::{Digest, Sha256};

static SEQ: AtomicU64 = AtomicU64::new(0);

#[path = "support/legacy_typescript_matrix.rs"]
mod legacy_typescript_matrix;

const INSTALL_ID: &str = "11111111-1111-4111-8111-111111111111";
const ORPHAN_INSTALL_ID: &str = "22222222-2222-4222-8222-222222222222";
const SECOND_ORPHAN_INSTALL_ID: &str = "33333333-3333-4333-8333-333333333333";
const ROLLED_BACK_INSTALL_ID: &str = "44444444-4444-4444-8444-444444444444";
const MISSING_CURRENT_INSTALL_ID: &str = "55555555-5555-4555-8555-555555555555";
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

struct LegacyTsV1JournalFixture {
    root: PathBuf,
    app: PathBuf,
    journal_path: PathBuf,
}

impl LegacyTsV1JournalFixture {
    fn create() -> Self {
        let root = temp_root();
        let app = root.join("ChatGPT.app");
        fs::create_dir_all(&app).unwrap();
        let state_root = root.join(".incodex");
        let journal_path = write_flat_journal(
            &state_root,
            &app,
            INSTALL_ID,
            "DISCOVERED",
            "2026-08-21T00:00:00.000Z",
        );
        Self {
            root: state_root,
            app,
            journal_path,
        }
    }

    fn advance_to(&self, phase: &str) {
        let phases = [
            "DISCOVERED",
            "BACKUP_COMMITTED",
            "STAGED",
            "PATCHED",
            "SIGNED",
            "VERIFIED",
            "TARGET_MOVED_OUT",
            "SWAPPED",
            "TARGET_VERIFIED",
            "ROLLED_BACK",
        ];
        let target_index = phases
            .iter()
            .position(|candidate| *candidate == phase)
            .unwrap();
        for next in phases.iter().skip(1).take(target_index) {
            if *next == "BACKUP_COMMITTED" {
                fs::create_dir_all(
                    self.root
                        .join("installations")
                        .join(target_id(&self.app))
                        .join(INSTALL_ID)
                        .join("original/ChatGPT.app"),
                )
                .unwrap();
            }
            if *next == "STAGED" {
                fs::create_dir_all(
                    self.root
                        .join("scratch")
                        .join(format!("ChatGPT.app.staged-{INSTALL_ID}")),
                )
                .unwrap();
            }
            if *next == "TARGET_MOVED_OUT" {
                fs::create_dir_all(
                    self.root
                        .join(format!("transactions/{INSTALL_ID}/outgoing/ChatGPT.app")),
                )
                .unwrap();
            }
            set_journal_phase_at(&self.journal_path, next);
        }
        if phase == "DISCOVERED" {
            return;
        }
        if phase == "ROLLED_BACK" {
            set_journal_phase_at(&self.journal_path, phase);
        }
    }
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

fn set_journal_phase_at(path: &std::path::Path, phase: &str) {
    let mut raw: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    raw["phase"] = json!(phase);
    fs::write(
        path,
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

fn write_flat_journal(
    root: &std::path::Path,
    app: &std::path::Path,
    install_id: &str,
    phase: &str,
    updated_at: &str,
) -> PathBuf {
    let target = canonical_path(app);
    let target_store = root.join("installations").join(target_id(app));
    let original = target_store.join(install_id).join("original/ChatGPT.app");
    let staged = if is_official_app(app, None) {
        root.join("ChatGPT.app.live")
    } else {
        root.join("scratch")
            .join(format!("ChatGPT.app.staged-{install_id}"))
    };
    let outgoing = root
        .join("transactions")
        .join(install_id)
        .join("outgoing/ChatGPT.app");
    let journal_path = root.join("transactions").join(format!("{install_id}.json"));
    fs::create_dir_all(journal_path.parent().unwrap()).unwrap();
    fs::write(
        &journal_path,
        format!(
            "{}\n",
            json!({
                "schemaVersion": 1,
                "installId": install_id,
                "targetRealPath": target,
                "stagedApp": staged,
                "originalSnapshot": original,
                "outgoingApp": outgoing,
                "phase": phase,
                "updatedAt": updated_at
            })
        ),
    )
    .unwrap();
    journal_path
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
    let incodex_cli::legacy_typescript::LegacyState::Committed {
        manifest,
        original_app,
        ..
    } = &state.state
    else {
        panic!("committed state should include manifest metadata");
    };

    assert_eq!(state.install_id, INSTALL_ID);
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.transaction_state, "committed");
    assert_eq!(
        manifest.original_plist_file_hash,
        sha256_hex(INFO_PLIST.as_bytes())
    );
    assert_eq!(state.journal.schema_version, 1);
    assert_eq!(state.journal.phase, "COMMITTED");
    assert_eq!(
        state.journal.original_snapshot,
        original_app.display().to_string()
    );
    assert_eq!(
        fs::read(original_app.join("Contents/Resources/app.asar")).unwrap(),
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
    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect_err("official live staging layout must not be accepted for a clone target");
    assert!(error.contains("stagedApp"), "{error}");

    let fixture = LegacyTsV1Fixture::create();
    incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect("clone scratch staging layout should be structurally valid")
        .expect("clone journal should be detected");

    let official_root = temp_root();
    write_flat_journal(
        &official_root,
        std::path::Path::new("/Applications/ChatGPT.app"),
        INSTALL_ID,
        "DISCOVERED",
        "2026-08-21T00:04:00.000Z",
    );
    incodex_cli::legacy_typescript::load_legacy_ts_v1(
        &official_root,
        std::path::Path::new("/Applications/ChatGPT.app"),
    )
    .expect("official live staging layout should be structurally valid")
    .expect("official journal should be detected");

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
fn legacy_typescript_fixture_rejects_current_pointer_without_a_committed_journal() {
    let fixture = LegacyTsV1Fixture::create();
    fs::remove_file(fixture.journal_path()).unwrap();
    write_flat_journal(
        &fixture.root,
        &fixture.app,
        ORPHAN_INSTALL_ID,
        "ROLLED_BACK",
        "2026-08-21T00:05:00.000Z",
    );

    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect_err("current metadata without its committed journal must fail closed");
    assert!(error.contains("current"), "{error}");
    assert!(error.contains(INSTALL_ID), "{error}");
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
fn legacy_typescript_fixture_reads_real_writer_order_before_installation_metadata() {
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
        let fixture = LegacyTsV1JournalFixture::create();
        fixture.advance_to(phase);
        let state = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
            .expect("early writer state should be structurally readable")
            .expect("journal must be detected before installation metadata exists");
        assert_eq!(state.install_id, INSTALL_ID, "phase {phase} was lost");
        assert_eq!(
            state.state.kind(),
            incodex_cli::legacy_typescript::LegacyStateKind::Interrupted,
            "phase {phase} was misclassified"
        );
        assert!(matches!(
            state.state,
            incodex_cli::legacy_typescript::LegacyState::Interrupted
        ));
    }
}

#[test]
fn legacy_typescript_fixture_reads_a_real_rolled_back_journal_without_metadata() {
    let fixture = LegacyTsV1JournalFixture::create();
    fixture.advance_to("ROLLED_BACK");
    let state = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect("rolled back writer state should be structurally readable")
        .expect("rolled back journal must be detected");
    assert_eq!(state.install_id, INSTALL_ID);
    assert_eq!(
        state.state.kind(),
        incodex_cli::legacy_typescript::LegacyStateKind::RolledBack
    );
    assert!(matches!(
        state.state,
        incodex_cli::legacy_typescript::LegacyState::RolledBack
    ));
}

#[test]
fn legacy_typescript_fixture_requires_metadata_for_a_committed_journal() {
    let fixture = LegacyTsV1JournalFixture::create();
    set_journal_phase_at(&fixture.journal_path, "COMMITTED");
    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect_err("committed journal without post-metadata records must be rejected");
    assert!(error.contains("metadata"), "{error}");
}

#[test]
fn legacy_typescript_fixture_does_not_attach_committed_metadata_to_an_interrupted_journal() {
    let fixture = LegacyTsV1Fixture::create();
    set_journal_phase_at(&fixture.journal_path(), "TARGET_VERIFIED");
    fs::remove_file(fixture.target_store().join("current.json")).unwrap();
    let state = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect("post-metadata interrupted state should be structurally readable")
        .expect("journal should be detected");
    assert_eq!(
        state.state.kind(),
        incodex_cli::legacy_typescript::LegacyStateKind::Interrupted
    );
    assert!(matches!(
        state.state,
        incodex_cli::legacy_typescript::LegacyState::Interrupted
    ));
}

#[test]
fn legacy_typescript_fixture_rejects_a_new_orphan_journal_next_to_an_old_committed_pointer() {
    let fixture = LegacyTsV1Fixture::create();
    let orphan = write_flat_journal(
        &fixture.root,
        &fixture.app,
        ORPHAN_INSTALL_ID,
        "DISCOVERED",
        "2026-08-21T00:01:00.000Z",
    );
    set_journal_phase_at(&orphan, "SWAPPED");

    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect_err("orphan journal and current committed pointer are ambiguous");
    assert!(error.contains(INSTALL_ID), "{error}");
    assert!(error.contains(ORPHAN_INSTALL_ID), "{error}");
}

#[test]
fn legacy_typescript_fixture_rejects_multiple_actionable_interrupted_journals() {
    let fixture = LegacyTsV1JournalFixture::create();
    let second = write_flat_journal(
        &fixture.root,
        &fixture.app,
        SECOND_ORPHAN_INSTALL_ID,
        "DISCOVERED",
        "2026-08-21T00:02:00.000Z",
    );
    set_journal_phase_at(&second, "SWAPPED");
    write_flat_journal(
        &fixture.root,
        &fixture.app,
        ROLLED_BACK_INSTALL_ID,
        "ROLLED_BACK",
        "2026-08-21T00:03:00.000Z",
    );

    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&fixture.root, &fixture.app)
        .expect_err("multiple actionable interrupted journals must fail closed");
    assert!(error.contains(INSTALL_ID), "{error}");
    assert!(error.contains(SECOND_ORPHAN_INSTALL_ID), "{error}");
    assert!(!error.contains(ROLLED_BACK_INSTALL_ID), "{error}");
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

#[test]
fn legacy_typescript_fixture_rejects_an_empty_manifest_target_before_canonicalizing() {
    let target = std::env::current_dir().expect("test working directory");
    let root = temp_root().join(".incodex");
    let target_store = root.join("installations").join(target_id(&target));
    let install_dir = target_store.join(INSTALL_ID);
    let original_app = install_dir.join("original/ChatGPT.app");
    fs::create_dir_all(&original_app).unwrap();
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
                "targetRealPath": canonical_path(&target),
                "bundleIdentifier": "com.example.incodex",
                "appVersion": "1.0.0",
                "appBuild": "100",
                "architecture": "arm64",
                "originalAsarHeaderHash": "original-header",
                "originalAsarFileHash": "original-file",
                "originalPlistFileHash": "original-plist",
                "patchedAsarHeaderHash": "patched-header",
                "patchedAsarFileHash": "patched-file",
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
                "patchedAsarHeaderHash": "patched-header",
                "patchedAsarFileHash": "patched-file"
            })
        ),
    )
    .unwrap();
    let journal_path = write_flat_journal(
        &root,
        &target,
        INSTALL_ID,
        "COMMITTED",
        "2026-08-20T00:00:00.000Z",
    );
    let manifest_path = install_dir.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["targetRealPath"] = json!("");
    fs::write(
        &manifest_path,
        format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap()),
    )
    .unwrap();

    let error = incodex_cli::legacy_typescript::load_legacy_ts_v1(&root, &target)
        .expect_err("empty manifest targetRealPath must be rejected before canonicalization");
    assert!(error.contains("targetRealPath"), "{error}");
    assert!(journal_path.exists());
}
