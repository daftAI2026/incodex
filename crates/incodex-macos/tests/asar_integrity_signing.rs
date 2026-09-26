#![cfg(target_os = "macos")]

use incodex_macos::{read_entitlements, sign_app_with_asar_integrity, verify_bundle_deep_strict};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

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

#[test]
fn signed_framework_digest_matches_the_updated_plist_without_disabling_validation() {
    let root = std::env::temp_dir().join(format!(
        "incodex-integrity-signing-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
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
    write_plist(&helper.join("Contents/Info.plist"), "LinkedHelper", "com.openai.codex.helper.fixture", "");
    let helper_source = root.join("helper.c");
    fs::write(&helper_source, "extern int fixture(void); int main(void) {return fixture()-1;}").unwrap();
    run("clang", &[helper_source.to_str().unwrap(), binary.to_str().unwrap(), "-o", helper.join("Contents/MacOS/LinkedHelper").to_str().unwrap()]);
    let entitlements = root.join("helper-entitlements.plist");
    fs::write(&entitlements, r#"<?xml version="1.0"?><plist><dict><key>com.apple.security.cs.allow-jit</key><true/></dict></plist>"#).unwrap();
    run("codesign", &["--force", "--sign", "-", "--options", "runtime", "--entitlements", entitlements.to_str().unwrap(), helper.to_str().unwrap()]);
    run(
        "codesign",
        &["--force", "--sign", "-", framework.to_str().unwrap()],
    );
    run(
        "codesign",
        &["--force", "--sign", "-", app.to_str().unwrap()],
    );
    sign_app_with_asar_integrity(&app, &"c".repeat(64)).unwrap();
    verify_bundle_deep_strict(&app).unwrap();
    let helper_entitlements = read_entitlements(&helper).unwrap();
    assert!(helper_entitlements.keys.contains("com.apple.security.cs.allow-jit"), "retain helper's original entitlements");
    assert!(helper_entitlements.keys.contains("com.apple.security.cs.disable-library-validation"), "a hardened helper loading the newly ad-hoc framework needs its own library-validation entitlement");
    let actual = fs::read(&binary).unwrap();
    let marker = b"AGbevlPCksUGKNL8TSn7wGmJEuJsXb2A";
    let offset = actual
        .windows(marker.len())
        .position(|slice| slice == marker)
        .unwrap();
    let slot = &actual[offset + 32..offset + 66];
    fs::remove_dir_all(&root).unwrap();
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
