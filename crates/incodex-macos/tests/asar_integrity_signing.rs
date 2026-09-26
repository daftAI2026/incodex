#![cfg(target_os = "macos")]

use incodex_macos::{read_entitlements, sign_app_with_asar_integrity, verify_bundle_deep_strict};
use std::{
    fs,
    path::Path,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn run(program: &str, args: &[&str]) {
    let output = Command::new(program).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{program}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn write_plist(path: &Path, executable: &str, identifier: &str, extra: &str) {
    fs::write(path, format!(r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>{executable}</string><key>CFBundleIdentifier</key><string>{identifier}</string><key>CFBundleVersion</key><string>1</string>{extra}</dict></plist>"#)).unwrap();
}

struct SignedFixture {
    root: std::path::PathBuf,
    app: std::path::PathBuf,
    framework: std::path::PathBuf,
    binary: std::path::PathBuf,
    helper: std::path::PathBuf,
    helper_binary: std::path::PathBuf,
}

fn signed_fixture(helper_identifier: &str) -> SignedFixture {
    let root = std::env::temp_dir().join(format!(
        "incodex-integrity-signing-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let app = root.join("ChatGPT.app");
    let framework = app.join("Contents/Frameworks/Renamed.framework");
    fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    fs::create_dir_all(framework.join("Resources")).unwrap();
    let digest = "6f22b7a4f82a2d9f48798c779ac3eec57d2cf91e549ce42866b193dd2ea3ec67";
    let bytes = (0..64)
        .step_by(2)
        .map(|i| format!("0x{}", &digest[i..i + 2]))
        .collect::<Vec<_>>()
        .join(",");
    let source = root.join("framework.c");
    fs::write(&source, format!(r#"__attribute__((used,section("__DATA_CONST,__asar_integrity"))) const struct {{ char sentinel[32]; unsigned char used, version, digest[32]; }} slot = {{"AGbevlPCksUGKNL8TSn7wGmJEuJsXb2A", 1, 1, {{{bytes}}}}}; int fixture(void) {{return 1;}}"#)).unwrap();
    let binary = framework.join("Renamed");
    run(
        "clang",
        &[
            "-dynamiclib",
            source.to_str().unwrap(),
            "-o",
            binary.to_str().unwrap(),
        ],
    );
    let main = root.join("main.c");
    fs::write(&main, "int main(void) {return 0;}").unwrap();
    run(
        "clang",
        &[
            main.to_str().unwrap(),
            "-o",
            app.join("Contents/MacOS/ChatGPT").to_str().unwrap(),
        ],
    );
    write_plist(
        &framework.join("Resources/Info.plist"),
        "Renamed",
        "com.example.framework",
        "",
    );
    let integrity = format!("<key>ElectronAsarIntegrity</key><dict><key>Resources/app.asar</key><dict><key>algorithm</key><string>SHA256</string><key>hash</key><string>{}</string></dict></dict>", "a".repeat(64));
    write_plist(
        &app.join("Contents/Info.plist"),
        "ChatGPT",
        "com.openai.codex",
        &integrity,
    );
    let helper = framework.join("Helpers/LinkedHelper.app");
    fs::create_dir_all(helper.join("Contents/MacOS")).unwrap();
    write_plist(
        &helper.join("Contents/Info.plist"),
        "LinkedHelper",
        helper_identifier,
        "",
    );
    let helper_source = root.join("helper.c");
    fs::write(
        &helper_source,
        "extern int fixture(void); int main(void) {return fixture()-1;}",
    )
    .unwrap();
    run(
        "clang",
        &[
            helper_source.to_str().unwrap(),
            binary.to_str().unwrap(),
            "-o",
            helper.join("Contents/MacOS/LinkedHelper").to_str().unwrap(),
        ],
    );
    let entitlements = root.join("helper-entitlements.plist");
    fs::write(&entitlements, r#"<?xml version="1.0"?><plist><dict><key>com.apple.security.cs.allow-jit</key><true/></dict></plist>"#).unwrap();
    run(
        "codesign",
        &[
            "--force",
            "--sign",
            "-",
            "--options",
            "runtime",
            "--entitlements",
            entitlements.to_str().unwrap(),
            helper.to_str().unwrap(),
        ],
    );
    run(
        "codesign",
        &["--force", "--sign", "-", framework.to_str().unwrap()],
    );
    run(
        "codesign",
        &["--force", "--sign", "-", app.to_str().unwrap()],
    );
    let helper_binary = helper.join("Contents/MacOS/LinkedHelper");
    SignedFixture {
        root,
        app,
        framework,
        binary,
        helper,
        helper_binary,
    }
}

fn code_directory_hash(path: &Path) -> String {
    let output = Command::new("codesign")
        .args(["--display", "--verbose=4", "--"])
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "codesign display {}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    let display = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    display
        .lines()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("CDHash=")
                .or_else(|| line.strip_prefix("CandidateCDHashFull sha256="))
                .or_else(|| line.strip_prefix("CandidateCDHash sha256="))
        })
        .expect("codesign output contains a CDHash")
        .to_owned()
}

#[test]
fn signed_framework_digest_matches_the_updated_plist_without_disabling_validation() {
    let fixture = signed_fixture("com.openai.codex.helper.fixture");
    sign_app_with_asar_integrity(&fixture.app, &"c".repeat(64)).unwrap();
    verify_bundle_deep_strict(&fixture.app).unwrap();
    let helper_entitlements = read_entitlements(&fixture.helper).unwrap();
    assert!(
        helper_entitlements
            .keys
            .contains("com.apple.security.cs.allow-jit"),
        "retain helper's original entitlements"
    );
    assert!(helper_entitlements.keys.contains("com.apple.security.cs.disable-library-validation"), "a hardened helper loading the newly ad-hoc framework needs its own library-validation entitlement");
    let actual = fs::read(&fixture.binary).unwrap();
    let marker = b"AGbevlPCksUGKNL8TSn7wGmJEuJsXb2A";
    let offset = actual
        .windows(marker.len())
        .position(|slice| slice == marker)
        .unwrap();
    let slot = &actual[offset + 32..offset + 66];
    fs::remove_dir_all(&fixture.root).unwrap();
    assert_eq!(
        &slot[..2],
        &[1, 1],
        "validation must remain enabled/version 1"
    );
    let expected = "aab91287495cc7a72a1a95c182a0a8be1025e4e8a8057d9812a743981bac2b62";
    let actual_digest = slot[2..]
        .iter()
        .map(|value| format!("{value:02x}"))
        .collect::<String>();
    assert_eq!(
        actual_digest, expected,
        "deep codesign success does not prove Electron integrity consistency"
    );
}

#[test]
fn unknown_direct_dependent_is_rejected_before_any_bundle_mutation() {
    let fixture = signed_fixture("com.openai.sky.fixture");
    let info_plist = fixture.app.join("Contents/Info.plist");
    let original_plist = fs::read(&info_plist).unwrap();
    let original_framework = fs::read(&fixture.binary).unwrap();
    let original_helper = fs::read(&fixture.helper_binary).unwrap();
    let original_hashes = [
        code_directory_hash(&fixture.app),
        code_directory_hash(&fixture.framework),
        code_directory_hash(&fixture.helper),
    ];

    let error = sign_app_with_asar_integrity(&fixture.app, &"c".repeat(64))
        .expect_err("unknown direct dependent must fail closed");
    assert!(error.contains("unknown helper"), "{error}");
    assert_eq!(fs::read(&info_plist).unwrap(), original_plist);
    assert_eq!(fs::read(&fixture.binary).unwrap(), original_framework);
    assert_eq!(fs::read(&fixture.helper_binary).unwrap(), original_helper);
    assert_eq!(
        [
            code_directory_hash(&fixture.app),
            code_directory_hash(&fixture.framework),
            code_directory_hash(&fixture.helper),
        ],
        original_hashes,
        "rejection must not alter any original code signature"
    );
    fs::remove_dir_all(&fixture.root).unwrap();
}
