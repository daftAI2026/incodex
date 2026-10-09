#![cfg(target_os = "macos")]

/**
 * [INPUT]: 依赖合成签名 fixture、普通 LC_LOAD_DYLIB 注入与完整性签名入口。
 * [OUTPUT]: 验证 provider 加载命令、Framework digest 与依赖 helper entitlement 共存。
 * [POS]: macOS 实验与指定基准的真实工具链合流证据，不访问用户 App 或 Keychain。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

#[allow(dead_code)]
#[path = "support/asar_integrity_fixture.rs"]
mod fixture;
use fixture::{run, signed_fixture_with_loader};
use incodex_macos::{read_entitlements, sign_app_with_asar_integrity, verify_bundle_deep_strict};
use std::fs;

#[test]
fn keychain_provider_and_framework_integrity_keep_helper_entitlements_and_load_command() {
    let fixture = signed_fixture_with_loader("com.openai.codex.helper.renderer", true);
    let framework = fixture
        .app
        .join("Contents/Frameworks/Codex Framework.framework");
    fs::rename(&fixture.framework, &framework).unwrap();
    let version = framework.join("Versions/A");
    fs::create_dir_all(&version).unwrap();
    for member in ["Renamed", "Resources", "Helpers", "_CodeSignature"] {
        if framework.join(member).exists() {
            fs::rename(framework.join(member), version.join(member)).unwrap();
        }
        if member != "_CodeSignature" {
            std::os::unix::fs::symlink(
                format!("Versions/Current/{member}"),
                framework.join(member),
            )
            .unwrap();
        }
    }
    std::os::unix::fs::symlink("A", framework.join("Versions/Current")).unwrap();
    fs::rename(version.join("Renamed"), version.join("Codex Framework")).unwrap();
    std::os::unix::fs::symlink("Codex Framework", version.join("Renamed")).unwrap();
    std::os::unix::fs::symlink(
        "Versions/Current/Codex Framework",
        framework.join("Codex Framework"),
    )
    .unwrap();
    fs::remove_file(framework.join("Renamed")).unwrap();
    let binary = framework.join("Codex Framework");
    let helper = framework.join(fixture.helper.strip_prefix(&fixture.framework).unwrap());
    run(
        "clang",
        &[
            "-dynamiclib",
            fixture.root.join("framework.c").to_str().unwrap(),
            "-Wl,-headerpad,0x200",
            "-o",
            version.join("Codex Framework").to_str().unwrap(),
        ],
    );
    let plist = framework.join("Resources/Info.plist");
    fs::write(
        &plist,
        fs::read_to_string(&plist)
            .unwrap()
            .replace(
                "<dict>",
                "<dict><key>CFBundlePackageType</key><string>FMWK</string>",
            )
            .replace(
                "<string>Renamed</string>",
                "<string>Codex Framework</string>",
            ),
    )
    .unwrap();
    let entitlement_file = fixture.root.join("provider-framework-entitlements.plist");
    fs::write(&entitlement_file, r#"<?xml version="1.0"?><plist><dict><key>com.apple.security.cs.allow-jit</key><true/></dict></plist>"#).unwrap();
    run(
        "codesign",
        &[
            "--force",
            "--sign",
            "-",
            "--options",
            "runtime",
            "--entitlements",
            entitlement_file.to_str().unwrap(),
            framework.to_str().unwrap(),
        ],
    );
    let provider = framework.join("Versions/Current/IncodexKeyProvider.dylib");
    fs::create_dir_all(provider.parent().unwrap()).unwrap();
    let provider_source = fixture.root.join("provider.c");
    fs::write(&provider_source, "int incodex_fixture(void) { return 1; }").unwrap();
    run(
        "clang",
        &[
            "-dynamiclib",
            provider_source.to_str().unwrap(),
            "-o",
            provider.to_str().unwrap(),
        ],
    );
    let load_path = "@loader_path/IncodexKeyProvider.dylib";
    std::os::unix::fs::symlink(
        "Versions/Current/IncodexKeyProvider.dylib",
        framework.join("IncodexKeyProvider.dylib"),
    )
    .unwrap();
    let mut injected = fs::read(&binary).unwrap();
    incodex_macos::add_load_dylib(&mut injected).unwrap();
    fs::write(&binary, injected).unwrap();

    sign_app_with_asar_integrity(&fixture.app, &"c".repeat(64)).unwrap();
    verify_bundle_deep_strict(&fixture.app).unwrap();
    let helper_entitlements = read_entitlements(&helper).unwrap();
    assert!(helper_entitlements
        .keys
        .contains("com.apple.security.cs.allow-jit"));
    assert!(helper_entitlements
        .keys
        .contains("com.apple.security.cs.disable-library-validation"));
    let bytes = fs::read(&binary).unwrap();
    assert!(
        bytes
            .windows(load_path.len())
            .any(|value| value == load_path.as_bytes()),
        "digest replacement must not discard the provider load command"
    );
    let marker = b"AGbevlPCksUGKNL8TSn7wGmJEuJsXb2A";
    let offset = bytes
        .windows(marker.len())
        .position(|value| value == marker)
        .unwrap();
    let expected = "aab91287495cc7a72a1a95c182a0a8be1025e4e8a8057d9812a743981bac2b62";
    let digest: String = bytes[offset + 34..offset + 66]
        .iter()
        .map(|value| format!("{value:02x}"))
        .collect();
    assert_eq!(digest, expected);
    fs::remove_dir_all(&fixture.root).unwrap();
}
