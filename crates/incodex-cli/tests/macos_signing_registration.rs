#![cfg(target_os = "macos")]

/**
 * [INPUT]: 依赖更新注册、私有签名身份注册与 codesign；仅在临时目录构造合成 app。
 * [OUTPUT]: 约束 schema2 的签名代际兼容、legacy None 不迁移与错误签名拒绝。
 * [POS]: macOS 更新注册的跨重启信任契约测试，不触碰官方 App、TCC 或用户 Keychain。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::macos_signing_assets::ensure_signing_identity;
use incodex_cli::macos_update_restore::{
    publish_registration, publish_registration_if_generation, read_registration,
    refresh_registered_helper, UpdateRegistration,
};

static SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    home: PathBuf,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let home = std::env::temp_dir().join(format!(
            "incodex-macos-signing-registration-{}-{}-{sequence}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        Self {
            root: home.join(".incodex"),
            home,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

#[test]
fn schema_two_registration_preserves_local_fingerprint_and_reads_old_missing_field_as_none() {
    let fixture = Fixture::new();
    let source = fixture.home.join("incodex-source");
    fs::create_dir_all(&fixture.home).unwrap();
    fs::write(&source, b"synthetic update helper").unwrap();

    // No registered signer means the publisher must not probe this absent app path.
    let app = fixture.home.join("not-installed/ChatGPT.app");
    let published = publish_registration(&fixture.root, &source, &app, "legacy-epoch").unwrap();
    let old_schema_two = serde_json::to_value(&published).unwrap();
    assert_eq!(old_schema_two["schemaVersion"], 2);
    assert!(old_schema_two.get("signingCertificateSha256").is_none());
    assert_eq!(
        read_registration(&fixture.root).unwrap().unwrap().app_path,
        app
    );

    // A schema-2 record from before the field existed must deserialize as legacy None, while
    // a present local fingerprint must survive deserialize/serialize instead of being erased.
    let fingerprint = "a".repeat(64);
    let mut local_schema_two = old_schema_two;
    local_schema_two["signingCertificateSha256"] = fingerprint.clone().into();
    let decoded: UpdateRegistration = serde_json::from_value(local_schema_two).unwrap();
    let encoded = serde_json::to_value(decoded).unwrap();
    assert_eq!(encoded["signingCertificateSha256"], fingerprint);
}

#[test]
fn legacy_adhoc_registration_never_migrates_when_a_root_identity_later_appears() {
    let fixture = Fixture::new();
    let old_source = fixture.home.join("incodex-old");
    let next_source = fixture.home.join("incodex-next");
    let recovery_source = fixture.home.join("incodex-recovery");
    fs::create_dir_all(&fixture.home).unwrap();
    fs::write(&old_source, b"old helper").unwrap();
    fs::write(&next_source, b"runtime refresh helper").unwrap();
    fs::write(&recovery_source, b"recovery helper").unwrap();
    let app = fixture.home.join("not-installed/ChatGPT.app");

    let legacy = publish_registration(&fixture.root, &old_source, &app, "legacy-epoch").unwrap();
    assert!(serde_json::to_value(&legacy)
        .unwrap()
        .get("signingCertificateSha256")
        .is_none());

    let identity = ensure_signing_identity(&fixture.root).unwrap();
    assert_eq!(identity.certificate_sha256.len(), 64);
    assert!(refresh_registered_helper(&fixture.root, &next_source).unwrap());
    let refreshed = read_registration(&fixture.root).unwrap().unwrap();
    assert!(serde_json::to_value(&refreshed)
        .unwrap()
        .get("signingCertificateSha256")
        .is_none());

    let recovered = publish_registration_if_generation(
        &fixture.root,
        &recovery_source,
        &app,
        "recovered-epoch",
        &refreshed.install_id,
        &refreshed.helper_sha256,
    )
    .unwrap();
    assert_eq!(recovered.install_id, "recovered-epoch");
    assert!(serde_json::to_value(recovered)
        .unwrap()
        .get("signingCertificateSha256")
        .is_none());
}

#[test]
fn publisher_rejects_an_adhoc_live_bundle_when_the_root_has_a_local_identity() {
    let fixture = Fixture::new();
    let source = fixture.home.join("incodex-source");
    let app = fixture.home.join("Applications/SyntheticHost.app");
    fs::create_dir_all(&fixture.home).unwrap();
    fs::write(&source, b"synthetic update helper").unwrap();
    let identity = ensure_signing_identity(&fixture.root).unwrap();
    assert_eq!(identity.certificate_sha256.len(), 64);
    create_adhoc_app(&app);

    let error = publish_registration(&fixture.root, &source, &app, "local-epoch").unwrap_err();

    assert!(
        error.contains("local") || error.contains("certificate"),
        "wrong signing proof should fail as an identity mismatch: {error}"
    );
    assert!(read_registration(&fixture.root).unwrap().is_none());
}

fn create_adhoc_app(app: &Path) {
    let executable = app.join("Contents/MacOS/Host");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    let source = app.with_extension("c");
    fs::write(&source, "int main(void) { return 0; }\n").unwrap();
    let output = Command::new("/usr/bin/clang")
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert_success("clang synthetic app", &output);
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).unwrap();
    fs::write(
        app.join("Contents/Info.plist"),
        br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Host</string>
<key>CFBundleIdentifier</key><string>org.incodex.synthetic.registration</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
</dict></plist>
"#,
    )
    .unwrap();
    let output = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-", "--"])
        .arg(app)
        .output()
        .unwrap();
    assert_success("codesign synthetic ad-hoc app", &output);
}

fn assert_success(label: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
