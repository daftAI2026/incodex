#![cfg(target_os = "macos")]

/**
 * [INPUT]: 依赖生产身份注册与 incodex-macos SigningContext，只在临时目录生成 synthetic bundles
 * [OUTPUT]: 约束 host/Sparkle/updater 组件各自绑定 identifier 与同一证书，跨不同载荷保持 DR
 * [POS]: RC2 签名拓扑门，不启动 GUI/真实 App，不触碰官方 CUA、真实 Keychain、trust 或 TCC
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_asar::{pack_dir, patch_asar};
use incodex_cli::diagnose::{diagnose_with_root_mode, DiagnosisMode};
use incodex_cli::macos_signing_assets::{ensure_signing_identity, unlock_signing_identity};
use incodex_macos::{
    ditto, sign_staged_app_with_context, verify_patched_bundle_with_context,
    LocalSigningIdentity, SigningContext,
};
use incodex_transaction::{validate_backup_snapshot, validate_committed_live_snapshot, Engine};

const HOST_IDENTIFIER: &str = "com.openai.codex";
const SPARKLE_IDENTIFIER: &str = "com.openai.codex.Sparkle";
const UPDATER_IDENTIFIER: &str = "com.openai.codex.Sparkle.Updater";
static SCRATCH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    home: PathBuf,
    root: PathBuf,
    app: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let sequence = SCRATCH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let home = std::env::temp_dir().join(format!(
            "incodex-macos-signing-context-{}-{}-{sequence}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        Self {
            root: home.join(".incodex"),
            app: home.join("active/SyntheticHost.app"),
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
fn production_context_signs_each_sparkle_domain_component_and_keeps_its_dr_across_payloads() {
    let fixture = Fixture::new("continuity");
    let identity = ensure_signing_identity(&fixture.root).unwrap();
    unlock_signing_identity(&fixture.root, &identity).unwrap();
    let context = SigningContext::Local(identity.clone());

    create_synthetic_host(&fixture.home, &fixture.app, 1);
    let v1 = commit_synthetic_install(&fixture.root, &fixture.home, &fixture.app, &context, 1);
    verify_patched_bundle_with_context(&fixture.app, None, &context).unwrap();
    let v1_requirements = component_requirements(&v1);
    assert_doctor_accepts_registered_local(&fixture.app, &fixture.root, &identity);

    let update = fixture.home.join("synthetic-official-update/ChatGPT.app");
    create_synthetic_host(&fixture.home, &update, 2);
    ditto(&update, &fixture.app).unwrap();
    let v2 = commit_synthetic_install(&fixture.root, &fixture.home, &fixture.app, &context, 2);
    verify_patched_bundle_with_context(&fixture.app, None, &context).unwrap();
    let v2_requirements = component_requirements(&v2);
    assert_doctor_accepts_registered_local(&fixture.app, &fixture.root, &identity);

    assert_eq!(v1.app, v2.app, "the synthetic installation path is stable");
    assert_ne!(v1.host_payload, v2.host_payload);
    assert_ne!(v1.sparkle_payload, v2.sparkle_payload);
    assert_ne!(v1.updater_payload, v2.updater_payload);
    assert_eq!(v1_requirements, v2_requirements);
    assert_requirement(&v2_requirements.host, &identity, HOST_IDENTIFIER);
    assert_requirement(&v2_requirements.sparkle, &identity, SPARKLE_IDENTIFIER);
    assert_requirement(&v2_requirements.updater, &identity, UPDATER_IDENTIFIER);

    let registration = fixture.root.join("macos-signing/identity.json");
    let good_metadata = fs::read(&registration).unwrap();
    fs::write(&registration, b"{ corrupted registration\n").unwrap();
    assert_doctor_rejects_unregistered_local(&fixture.app, &fixture.root);
    fs::write(&registration, good_metadata).unwrap();
    fs::remove_file(&registration).unwrap();
    assert_doctor_rejects_unregistered_local(&fixture.app, &fixture.root);
}

#[test]
fn context_verifier_rejects_a_different_certificate_and_identifier_only_dr() {
    let fixture = Fixture::new("rejection");
    let identity = ensure_signing_identity(&fixture.root).unwrap();
    unlock_signing_identity(&fixture.root, &identity).unwrap();
    let context = SigningContext::Local(identity.clone());
    create_synthetic_host(&fixture.home, &fixture.app, 3);
    let components = commit_synthetic_install(&fixture.root, &fixture.home, &fixture.app, &context, 3);
    verify_patched_bundle_with_context(&fixture.app, None, &context).unwrap();

    let wrong_root = fixture.home.join(".incodex-wrong-certificate");
    let wrong_identity = ensure_signing_identity(&wrong_root).unwrap();
    unlock_signing_identity(&wrong_root, &wrong_identity).unwrap();
    assert_ne!(identity.certificate_sha1, wrong_identity.certificate_sha1);
    assert!(verify_patched_bundle_with_context(
        &fixture.app,
        None,
        &SigningContext::Local(wrong_identity)
    )
    .is_err());

    sign_identifier_only(&fixture.app, &identity, HOST_IDENTIFIER);
    assert!(verify_patched_bundle_with_context(&fixture.app, None, &context).is_err());

    // Keep this binding explicit: the rejected object is the same signed host,
    // not a verifier failure caused by a missing or malformed synthetic bundle.
    assert!(components.host.exists());
    assert!(components.updater.exists());
}

struct SyntheticComponents {
    app: PathBuf,
    host: PathBuf,
    sparkle: PathBuf,
    updater: PathBuf,
    host_payload: Vec<u8>,
    sparkle_payload: Vec<u8>,
    updater_payload: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
struct ComponentRequirements {
    host: String,
    sparkle: String,
    updater: String,
}

fn create_synthetic_host(home: &Path, app: &Path, generation: u8) {
    let host = app.join("Contents/MacOS/Host");
    let sparkle = app.join("Contents/Frameworks/Sparkle.framework");
    let sparkle_binary = sparkle.join("Sparkle");
    let updater = sparkle.join("Helpers/Updater.app");
    let updater_binary = updater.join("Contents/MacOS/Updater");
    fs::create_dir_all(host.parent().unwrap()).unwrap();
    fs::create_dir_all(sparkle.join("Resources")).unwrap();
    fs::create_dir_all(updater_binary.parent().unwrap()).unwrap();
    let resources = app.join("Contents/Resources");
    let package_source = home.join(format!("package-source-v{generation}"));
    fs::create_dir_all(&resources).unwrap();
    fs::create_dir_all(&package_source).unwrap();
    fs::write(
        package_source.join("package.json"),
        r#"{"name":"synthetic-host","version":"1.0.0","main":"index.js"}"#,
    )
    .unwrap();
    fs::write(
        package_source.join("index.js"),
        format!("module.exports = 'payload-v{generation}';\n"),
    )
    .unwrap();
    let asar = resources.join("app.asar");
    pack_dir(&package_source, &asar).unwrap();

    write_info_plist(
        &app.join("Contents/Info.plist"),
        "Host",
        HOST_IDENTIFIER,
        "",
    );
    write_info_plist(
        &sparkle.join("Resources/Info.plist"),
        "Sparkle",
        SPARKLE_IDENTIFIER,
        "",
    );
    write_info_plist(
        &updater.join("Contents/Info.plist"),
        "Updater",
        UPDATER_IDENTIFIER,
        "",
    );

    let host_source = home.join(format!("host-v{generation}.c"));
    let sparkle_source = home.join(format!("sparkle-v{generation}.c"));
    let updater_source = home.join(format!("updater-v{generation}.c"));
    let host_body = format!("int main(void) {{ return {}; }}\n", generation % 2);
    let sparkle_body = format!("int sparkle_payload(void) {{ return {generation}; }}\n");
    let updater_body = format!("int main(void) {{ return {}; }}\n", generation % 2);
    fs::write(&host_source, &host_body).unwrap();
    fs::write(&sparkle_source, &sparkle_body).unwrap();
    fs::write(&updater_source, &updater_body).unwrap();
    run(
        "/usr/bin/clang",
        &[
            host_source.to_str().unwrap(),
            "-o",
            host.to_str().unwrap(),
        ],
    );
    run(
        "/usr/bin/clang",
        &[
            "-dynamiclib",
            sparkle_source.to_str().unwrap(),
            "-o",
            sparkle_binary.to_str().unwrap(),
        ],
    );
    run(
        "/usr/bin/clang",
        &[
            updater_source.to_str().unwrap(),
            "-o",
            updater_binary.to_str().unwrap(),
        ],
    );

    for bundle in [&updater, &sparkle, app] {
        run(
            "/usr/bin/codesign",
            &[
                "--force",
                "--sign",
                "-",
                "--",
                bundle.to_str().unwrap(),
            ],
        );
    }
}

fn commit_synthetic_install(
    root: &Path,
    home: &Path,
    app: &Path,
    context: &SigningContext,
    generation: u8,
) -> SyntheticComponents {
    let mut transaction = Engine::begin(root, app, "synthetic-signing-install").unwrap();
    let install_id = transaction.install_id().to_string();
    let original = root
        .join("transactions")
        .join(&install_id)
        .join("original")
        .join(app.file_name().unwrap());
    ditto(app, &original).unwrap();
    transaction.mark_backup_committed().unwrap();

    let staged = home.join(format!("candidate-v{generation}.app"));
    ditto(app, &staged).unwrap();
    let asar = staged.join("Contents/Resources/app.asar");
    let (asar_hash, _) = patch_asar(
        &asar,
        incodex_runtime_bundle::loader_source(),
        Some(transaction.install_id()),
    )
    .unwrap();
    sign_staged_app_with_context(&staged, app, &asar_hash, context).unwrap();
    transaction.place_staging(&staged).unwrap();
    transaction.swap().unwrap();
    transaction.commit().unwrap();

    validate_committed_live_snapshot(root, &install_id, app).unwrap();
    validate_backup_snapshot(root, &install_id).unwrap();
    let host = app.join("Contents/MacOS/Host");
    let sparkle = app.join("Contents/Frameworks/Sparkle.framework");
    let updater = sparkle.join("Helpers/Updater.app");
    let sparkle_binary = sparkle.join("Sparkle");
    let updater_binary = updater.join("Contents/MacOS/Updater");
    SyntheticComponents {
        app: app.to_path_buf(),
        host_payload: fs::read(&host).unwrap(),
        sparkle_payload: fs::read(&sparkle_binary).unwrap(),
        updater_payload: fs::read(&updater_binary).unwrap(),
        host,
        sparkle,
        updater,
    }
}

fn assert_doctor_accepts_registered_local(app: &Path, root: &Path, identity: &LocalSigningIdentity) {
    for mode in [DiagnosisMode::Doctor, DiagnosisMode::DoctorDeep] {
        let report = serde_json::to_value(diagnose_with_root_mode(app, root, mode)).unwrap();
        assert_eq!(report["codesignOk"], true, "mode={mode:?}");
        assert_eq!(
            report["signing"]["registeredLocalIdentity"]["certificateSha1"],
            identity.certificate_sha1,
            "mode={mode:?}"
        );
        assert_eq!(
            report["signing"]["registeredLocalIdentity"]["certificateSha256"],
            identity.certificate_sha256,
            "mode={mode:?}"
        );
        assert_eq!(
            report["signing"]["registeredLocalIdentity"]["matched"],
            true,
            "mode={mode:?}"
        );
    }
}

fn assert_doctor_rejects_unregistered_local(app: &Path, root: &Path) {
    for mode in [DiagnosisMode::Doctor, DiagnosisMode::DoctorDeep] {
        let report = serde_json::to_value(diagnose_with_root_mode(app, root, mode)).unwrap();
        assert_eq!(report["codesignOk"], false, "mode={mode:?}");
        assert_ne!(
            report["signing"]["registeredLocalIdentity"]["matched"],
            true,
            "an absent or corrupt registration cannot accept the local signature"
        );
    }
}

fn component_requirements(components: &SyntheticComponents) -> ComponentRequirements {
    for component in [&components.app, &components.sparkle, &components.updater] {
        assert!(
            verify_codesign(component).status.success(),
            "signed component must remain strictly verifiable: {}",
            component.display()
        );
    }
    ComponentRequirements {
        host: designated_requirement(&components.app),
        sparkle: designated_requirement(&components.sparkle),
        updater: designated_requirement(&components.updater),
    }
}

fn assert_requirement(actual: &str, identity: &LocalSigningIdentity, identifier: &str) {
    assert_eq!(actual, identity.requirement(identifier).unwrap());
    assert!(actual.contains(&identity.certificate_sha1));
}

fn designated_requirement(component: &Path) -> String {
    let output = Command::new("/usr/bin/codesign")
        .args(["--display", "--requirements", "-", "--"])
        .arg(component)
        .output()
        .unwrap();
    assert_success("codesign requirement inspection", output.status.success(), &output);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    text.lines()
        .map(str::trim)
        .find(|line| line.starts_with("designated => "))
        .expect("component must carry an explicit designated requirement")
        .to_string()
}

fn sign_identifier_only(app: &Path, identity: &LocalSigningIdentity, identifier: &str) {
    let weak_requirement = format!("=designated => identifier \"{identifier}\"");
    let output = Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", &identity.certificate_sha1, "--keychain"])
        .arg(&identity.keychain_path)
        .args(["--timestamp=none", "--identifier", identifier, "--requirements"])
        .arg(weak_requirement)
        .args(["--", app.to_str().unwrap()])
        .output()
        .unwrap();
    assert_success("identifier-only synthetic codesign", output.status.success(), &output);
}

fn write_info_plist(path: &Path, executable: &str, identifier: &str, extra: &str) {
    fs::write(
        path,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>{executable}</string><key>CFBundleIdentifier</key><string>{identifier}</string><key>CFBundleVersion</key><string>1</string>{extra}</dict></plist>"#
        ),
    )
    .unwrap();
}

fn run(program: &str, args: &[&str]) {
    let output = Command::new(program).args(args).output().unwrap();
    assert_success(program, output.status.success(), &output);
}

fn verify_codesign(component: &Path) -> Output {
    Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict", "--deep", "--"])
        .arg(component)
        .output()
        .unwrap()
}

fn assert_success(label: &str, success: bool, output: &Output) {
    assert!(
        success,
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
