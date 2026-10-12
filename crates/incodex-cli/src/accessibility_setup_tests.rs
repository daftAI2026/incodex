#![cfg(target_os = "macos")]
/*
 * [INPUT]: 依赖真实权限请求生产器与隔离 COMMITTED 事务。
 * [OUTPUT]: 验证生产请求交接、规范夹具路径与私有 marker。
 * [POS]: CLI 权限交接回归；不调用真实 App 或 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

use super::request_setup;
use incodex_macos::ditto;
use incodex_transaction::Engine;
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    sandbox: PathBuf,
    root: PathBuf,
    app: PathBuf,
    install_id: String,
}

impl Fixture {
    fn transaction_dir(&self) -> PathBuf {
        self.root.join("transactions").join(&self.install_id)
    }

    fn marker_path(&self) -> PathBuf {
        self.transaction_dir().join("accessibility-setup.json")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.sandbox);
    }
}

fn committed_fixture(label: &str) -> Fixture {
    let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let sandbox = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "incodex-accessibility-setup-{label}-{}-{sequence}",
        std::process::id()
    ));
    let root = sandbox.join(".incodex");
    let app = sandbox.join("ChatGPT.app");
    fs::create_dir_all(app.join("Contents/Resources")).unwrap();
    fs::write(app.join("Contents/Info.plist"), b"fixture\n").unwrap();
    fs::write(app.join("Contents/Resources/payload"), b"official\n").unwrap();
    let asar_source = sandbox.join("asar-source");
    fs::create_dir(&asar_source).unwrap();
    fs::write(asar_source.join("package.json"), br#"{"main":"main.js"}"#).unwrap();
    fs::write(asar_source.join("main.js"), b"module.exports = {};\n").unwrap();
    incodex_asar::pack_dir(&asar_source, &app.join("Contents/Resources/app.asar")).unwrap();

    let mut transaction = Engine::begin(&root, &app, "accessibility-setup-test").unwrap();
    let install_id = transaction.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&install_id)
        .join("original/ChatGPT.app");
    fs::create_dir_all(original.parent().unwrap()).unwrap();
    ditto(&app, &original).unwrap();
    transaction.mark_backup_committed().unwrap();

    let candidate = sandbox.join("candidate.app");
    ditto(&app, &candidate).unwrap();
    fs::write(candidate.join("Contents/Resources/payload"), b"installed\n").unwrap();
    incodex_asar::patch_asar(
        &candidate.join("Contents/Resources/app.asar"),
        "module.exports = {};\n",
        Some(&install_id),
    )
    .unwrap();
    transaction.place_staging(&candidate).unwrap();
    transaction.swap().unwrap();
    transaction.commit().unwrap();

    // The production path expects the full transaction ancestry to be private
    // before it considers placing a Runtime coordination marker.
    let transactions = root.join("transactions");
    let transaction_dir = transactions.join(&install_id);
    for path in [
        root.as_path(),
        transactions.as_path(),
        transaction_dir.as_path(),
    ] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    Fixture {
        sandbox,
        root,
        app,
        install_id,
    }
}

fn read_marker(fixture: &Fixture) -> Value {
    serde_json::from_slice(&fs::read(fixture.marker_path()).unwrap()).unwrap()
}

#[test]
fn committed_fixture_uses_canonical_paths_for_the_bound_app() {
    let fixture = committed_fixture("canonical-paths");
    assert_eq!(fixture.sandbox, fixture.sandbox.canonicalize().unwrap());
    assert_eq!(fixture.root, fixture.root.canonicalize().unwrap());
    assert_eq!(fixture.app, fixture.app.canonicalize().unwrap());
}

#[test]
fn new_install_assigns_presentation_to_the_shared_cli_host() {
    let fixture = committed_fixture("cli-presentation");
    request_setup(&fixture.root, &fixture.app, &fixture.install_id).unwrap();
    assert_eq!(read_marker(&fixture)["presentationOwner"], "cli");
}

#[test]
fn production_request_id_uses_uuid_v4_without_rotating_installation_identity() {
    let fixture = committed_fixture("uuid-wire");
    let original_install = fixture.install_id.clone();
    let first = request_setup(&fixture.root, &fixture.app, &fixture.install_id).unwrap();
    let second = request_setup(&fixture.root, &fixture.app, &fixture.install_id).unwrap();
    assert_eq!(read_marker(&fixture)["requestId"], second);
    assert_ne!(first, second);
    for request in [first, second] {
        assert_eq!(request.len(), 36, "the running host expects UUID requests");
        assert!(super::is_uuid(&request));
        assert_eq!(&request[14..15], "4");
        assert!(matches!(&request[19..20], "8" | "9" | "a" | "b"));
    }
    assert_eq!(read_marker(&fixture)["installId"], original_install);
    assert_eq!(read_marker(&fixture)["presentationOwner"], "cli");
}

#[test]
fn production_request_id_interoperates_with_frozen_runtime_responder() {
    let fixture = committed_fixture("frozen-wire");
    let request = request_setup(&fixture.root, &fixture.app, &fixture.install_id).unwrap();
    assert_eq!(read_marker(&fixture)["requestId"], request);
    let responder =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/permission-responder-rc4.cjs");
    // 保留冻结 IIFE 的原始字节。此回归只证明 requestId 格式，不伪装真实 TCC。
    use sha2::Digest;
    let source = fs::read_to_string(&responder).unwrap();
    let expression = source
        .split_once("module.exports = ")
        .unwrap()
        .1
        .split_once("\n\nif (require.main")
        .unwrap()
        .0
        .trim_end_matches(';');
    assert_eq!(
        sha2::Sha256::digest(expression.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        "0021b3309b1def9469589cc92e82bba35ba44629a02e84dab9eb8d027ab58e4e"
    );
    let result = std::process::Command::new("bun")
        .arg(responder)
        .arg(request)
        .arg(&fixture.sandbox)
        .arg(&fixture.install_id)
        .output()
        .expect("cross-language Runtime regression requires the repository Bun toolchain");
    assert!(
        result.status.success(),
        "frozen Runtime rejected the production request: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(receipt["result"], "PASS");
    assert_eq!(
        receipt["observations"],
        json!(["unknown", "granted", "not-granted", "unknown"])
    );
}

#[test]
fn real_setup_request_is_accepted_by_installed_screen_probe() {
    let fixture = committed_fixture("screen-request-producer");
    let request_id = request_setup(&fixture.root, &fixture.app, &fixture.install_id).unwrap();
    assert_eq!(request_id.len(), 36);
    assert!(super::is_uuid(&request_id));
    assert!(
        crate::permission_screen_probe::InstalledScreenProbe::for_app(&fixture.root, &fixture.app)
            .is_some()
    );

    let mut marker = read_marker(&fixture);
    for invalid in [
        "22222222-2222-4222-7222-222222222222",
        "22222222-2222-0222-8222-222222222222",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "2222222222222222222222222222222",
        "gggggggggggggggggggggggggggggggg",
    ] {
        marker["requestId"] = json!(invalid);
        write_marker(&fixture, &marker);
        assert!(
            crate::permission_screen_probe::InstalledScreenProbe::for_app(
                &fixture.root,
                &fixture.app
            )
            .is_none(),
            "invalid request identity was accepted: {invalid}"
        );
    }
    marker["requestId"] = json!("2".repeat(32));
    write_marker(&fixture, &marker);
    assert!(
        crate::permission_screen_probe::InstalledScreenProbe::for_app(&fixture.root, &fixture.app)
            .is_some(),
        "historical marker remains readable, not proof of old responder interoperability"
    );
    marker["requestId"] = json!(request_id);
    write_marker(&fixture, &marker);
    assert!(
        crate::permission_screen_probe::InstalledScreenProbe::for_app(&fixture.root, &fixture.app)
            .is_some()
    );
    marker["state"] = json!("granted");
    write_marker(&fixture, &marker);
    assert!(
        crate::permission_screen_probe::InstalledScreenProbe::for_app(&fixture.root, &fixture.app)
            .is_none()
    );
}

#[test]
fn cli_completion_is_bound_to_the_current_request_and_preserves_ownership() {
    let fixture = committed_fixture("cli-completion");
    request_setup(&fixture.root, &fixture.app, &fixture.install_id).unwrap();
    let request = read_marker(&fixture)["requestId"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(super::finish_cli_setup(
        &fixture.root,
        &fixture.app,
        &fixture.install_id,
        "old-request",
        "granted"
    )
    .is_err());
    assert_eq!(read_marker(&fixture)["state"], "pending");
    super::finish_cli_setup(
        &fixture.root,
        &fixture.app,
        &fixture.install_id,
        &request,
        "granted",
    )
    .unwrap();
    let marker = read_marker(&fixture);
    assert_eq!(marker["state"], "granted");
    assert_eq!(marker["presentationOwner"], "cli");
    assert!(marker["updatedAtMs"].as_u64().unwrap() > 0);
}

#[test]
fn each_explicit_install_has_a_distinct_setup_request() {
    let fixture = committed_fixture("request-generation");
    request_setup(&fixture.root, &fixture.app, &fixture.install_id).unwrap();
    let first = read_marker(&fixture);
    request_setup(&fixture.root, &fixture.app, &fixture.install_id).unwrap();
    let second = read_marker(&fixture);
    assert!(first["requestId"].as_str().is_some());
    assert_ne!(first["requestId"], second["requestId"]);
}

fn write_marker(fixture: &Fixture, value: &Value) {
    fs::write(fixture.marker_path(), serde_json::to_vec(value).unwrap()).unwrap();
    fs::set_permissions(fixture.marker_path(), fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn request_writes_pending_marker_with_canonical_app_binding() {
    let fixture = committed_fixture("pending");
    let app_alias = fixture
        .sandbox
        .join("nested")
        .join("..")
        .join("ChatGPT.app");
    fs::create_dir_all(fixture.sandbox.join("nested")).unwrap();

    let app_before = fs::read(fixture.app.join("Contents/Resources/payload")).unwrap();
    request_setup(&fixture.root, &app_alias, &fixture.install_id).unwrap();

    let marker = read_marker(&fixture);
    assert_eq!(marker["schemaVersion"], 1);
    assert_eq!(marker["installId"], fixture.install_id);
    assert_eq!(
        marker["appPath"],
        fixture.app.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(marker["state"], "pending");
    assert!(marker["requestedAtMs"].as_u64().is_some());
    assert_eq!(
        fs::metadata(fixture.marker_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::read(fixture.app.join("Contents/Resources/payload")).unwrap(),
        app_before
    );
}

#[test]
fn request_rearms_only_existing_granted_or_deferred_marker() {
    for (state, label) in [("granted", "granted"), ("deferred", "deferred")] {
        let fixture = committed_fixture(label);
        write_marker(
            &fixture,
            &json!({
                "schemaVersion": 1,
                "installId": fixture.install_id,
                "appPath": fixture.app.canonicalize().unwrap().display().to_string(),
                "requestedAtMs": 1,
                "state": state,
                "checkedAtMs": 2,
                "pid": 42,
                "reason": "fixture"
            }),
        );

        request_setup(&fixture.root, &fixture.app, &fixture.install_id).unwrap();

        let marker = read_marker(&fixture);
        assert_eq!(marker["schemaVersion"], 1);
        assert_eq!(marker["installId"], fixture.install_id);
        assert_eq!(marker["state"], "pending");
        assert!(marker["requestedAtMs"].as_u64().is_some());
    }
}

#[test]
fn malformed_existing_marker_is_preserved_and_fails_closed() {
    let fixture = committed_fixture("malformed");
    let malformed = br#"{"schemaVersion":1,"state":"pending""#;
    fs::write(fixture.marker_path(), malformed).unwrap();
    fs::set_permissions(fixture.marker_path(), fs::Permissions::from_mode(0o600)).unwrap();

    let error = request_setup(&fixture.root, &fixture.app, &fixture.install_id).unwrap_err();

    assert!(!error.is_empty());
    assert_eq!(fs::read(fixture.marker_path()).unwrap(), malformed);
}

#[test]
fn existing_marker_symlink_is_rejected_without_following_or_replacing_it() {
    let fixture = committed_fixture("marker-symlink");
    let outside = fixture.sandbox.join("outside-marker.json");
    fs::write(&outside, b"outside-sentinel\n").unwrap();
    symlink(&outside, fixture.marker_path()).unwrap();

    let error = request_setup(&fixture.root, &fixture.app, &fixture.install_id).unwrap_err();

    assert!(!error.is_empty());
    assert!(fs::symlink_metadata(fixture.marker_path())
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(fs::read(outside).unwrap(), b"outside-sentinel\n");
}

#[test]
fn existing_marker_nonregular_file_is_rejected() {
    let fixture = committed_fixture("marker-directory");
    fs::create_dir(fixture.marker_path()).unwrap();

    let error = request_setup(&fixture.root, &fixture.app, &fixture.install_id).unwrap_err();

    assert!(!error.is_empty());
    assert!(fs::symlink_metadata(fixture.marker_path())
        .unwrap()
        .file_type()
        .is_dir());
}

#[test]
fn request_rejects_missing_transaction_without_creating_its_directory() {
    let fixture = committed_fixture("missing-transaction");
    let missing_id = "0f5e8d25-0bb0-4c88-8ddc-3c4b8c5e0b91";
    let missing_transaction = fixture.root.join("transactions").join(missing_id);

    let error = request_setup(&fixture.root, &fixture.app, missing_id).unwrap_err();

    assert!(!error.is_empty());
    assert!(!missing_transaction.exists());
    assert!(!missing_transaction
        .join("accessibility-setup.json")
        .exists());
}

#[test]
fn request_rejects_invalid_id_relative_or_symlinked_app_without_marker() {
    let fixture = committed_fixture("input-guards");
    let relative_app = Path::new("ChatGPT.app");
    assert!(request_setup(&fixture.root, relative_app, &fixture.install_id).is_err());
    assert!(!fixture.marker_path().exists());

    assert!(request_setup(&fixture.root, &fixture.app, "not-an-uuid").is_err());
    assert!(!fixture.marker_path().exists());

    let app_alias = fixture.sandbox.join("ChatGPT-alias.app");
    symlink(&fixture.app, &app_alias).unwrap();
    assert!(request_setup(&fixture.root, &app_alias, &fixture.install_id).is_err());
    assert!(!fixture.marker_path().exists());
}

#[test]
fn request_rejects_foreign_app_even_when_transaction_id_is_valid() {
    let fixture = committed_fixture("foreign-app");
    let foreign_app = fixture.sandbox.join("Foreign.app");
    fs::create_dir_all(&foreign_app).unwrap();
    fs::write(foreign_app.join("marker"), b"foreign\n").unwrap();

    let error = request_setup(&fixture.root, &foreign_app, &fixture.install_id).unwrap_err();

    assert!(!error.is_empty());
    assert!(!fixture.marker_path().exists());
}

#[test]
fn request_rejects_group_or_world_writable_transaction_ancestry() {
    for (mode, label) in [(0o0770, "group"), (0o0707, "world")] {
        let fixture = committed_fixture(label);
        let transactions = fixture.root.join("transactions");
        fs::set_permissions(&transactions, fs::Permissions::from_mode(mode)).unwrap();

        let error = request_setup(&fixture.root, &fixture.app, &fixture.install_id).unwrap_err();

        assert!(!error.is_empty());
        assert!(!fixture.marker_path().exists());
    }
}

#[test]
fn request_rejects_symlinked_transaction_ancestry_without_following_it() {
    let fixture = committed_fixture("transaction-symlink");
    let real_transaction = fixture.transaction_dir();
    let moved_transaction = fixture.sandbox.join("moved-transaction");
    fs::rename(&real_transaction, &moved_transaction).unwrap();
    symlink(&moved_transaction, &real_transaction).unwrap();

    let error = request_setup(&fixture.root, &fixture.app, &fixture.install_id).unwrap_err();

    assert!(!error.is_empty());
    assert!(fs::symlink_metadata(&real_transaction)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(!moved_transaction.join("accessibility-setup.json").exists());
}
