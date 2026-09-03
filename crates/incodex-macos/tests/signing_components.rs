//! [INPUT]: 依赖 incodex-macos 的签名清单与重签入口，以可控的 codesign 替身模拟组件身份和签名失效。
//! [OUTPUT]: 验证 CUA/vendor sidecar 保留、Sparkle 同代重签，以及 Provider 所在 Framework 与 Electron helpers 同代重签。
//! [POS]: incodex-macos 的组件级签名回归套件，约束 install 在修改官方 bundle 后仍能生成 deep/strict 可验收产物。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_macos::{collect_vendor_helper_roots, sign_app};

static PATH_LOCK: Mutex<()> = Mutex::new(());

struct PathGuard(Option<OsString>);

impl Drop for PathGuard {
    fn drop(&mut self) {
        match &self.0 {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
    }
}

struct Fixture {
    root: PathBuf,
    app: PathBuf,
    sidecar: PathBuf,
    fake_bin: PathBuf,
    entitlements: PathBuf,
    marker: PathBuf,
    sparkle: PathBuf,
    codex_framework: PathBuf,
    keychain_provider: PathBuf,
    codex_helper_marker: PathBuf,
    codex_bare_helper_marker: PathBuf,
    framework_sign_state: PathBuf,
    sign_capture: PathBuf,
    deep_capture: PathBuf,
}

impl Fixture {
    fn new(identity: &str) -> Self {
        Self::with_outer(identity, false)
    }

    fn new_custom_outer(identity: &str) -> Self {
        Self::with_outer(identity, true)
    }

    fn with_outer(identity: &str, custom_outer: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "incodex-signing-components-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let app = root.join("ChatGPT.app");
        let sidecar = app.join("Contents/Frameworks/RenamedVendor.xpc");
        let sparkle = app.join("Contents/Frameworks/Sparkle.framework");
        let codex_framework = app.join("Contents/Frameworks/Codex Framework.framework");
        let keychain_provider = codex_framework.join("Versions/Current/IncodexKeyProvider.dylib");
        let codex_helper_marker = codex_framework
            .join("Versions/Current/Helpers/Codex (Renderer).app/Contents/vendor-marker");
        let codex_bare_helper_marker =
            codex_framework.join("Versions/Current/Helpers/browser_crashpad_handler");
        let fake_bin = root.join("fake-bin");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::create_dir_all(sidecar.join("Contents/_CodeSignature")).unwrap();
        fs::create_dir_all(sparkle.join("Versions/B")).unwrap();
        fs::create_dir_all(&fake_bin).unwrap();
        fs::write(app.join("Contents/MacOS/ChatGPT"), "binary\n").unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.openai.codex</string>
<key>CFBundleShortVersionString</key><string>1.0.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleExecutable</key><string>ChatGPT</string>
</dict></plist>
"#,
        )
        .unwrap();
        let marker = sidecar.join("Contents/vendor-marker");
        fs::write(&marker, "original-vendor-component\n").unwrap();
        let entitlements = root.join("host-entitlements.plist");
        fs::write(
            &entitlements,
            r#"<?xml version="1.0"?><plist><dict><key>com.apple.security.cs.allow-jit</key><true/></dict></plist>"#,
        )
        .unwrap();
        let sign_capture = root.join("outer-sign-count");
        let deep_capture = root.join("deep-sign-count");
        let framework_sign_state = root.join("codex-framework-signed");
        let nested_display = if custom_outer {
            format!(
                "if [ -f \"$INCODEX_DEEP_SIGN_CAPTURE\" ]; then printf '%s\\n' 'Identifier=com.example.fixture' 'Signature=adhoc'; else printf '%s\\n' 'Identifier=com.example.renamed-vendor' 'TeamIdentifier={identity}' 'Authority=Developer ID Application: fixture'; fi"
            )
        } else {
            format!(
                "printf '%s\\n' 'Identifier=com.example.renamed-vendor' 'TeamIdentifier={identity}' 'Authority=Developer ID Application: fixture'"
            )
        };
        let outer_display = if custom_outer {
            "if [ -f \"$INCODEX_SIGN_CAPTURE\" ]; then printf '%s\\n' 'Identifier=com.example.fixture' 'Signature=adhoc'; else printf '%s\\n' 'Identifier=com.example.third-party' 'TeamIdentifier=THIRDPARTY' 'Authority=Developer ID Application: third-party fixture'; fi"
        } else {
            "printf '%s\\n' 'Identifier=com.openai.codex' 'Signature=adhoc'"
        };
        let script = format!(
            r#"#!/bin/sh
target=""
for arg in "$@"; do target="$arg"; done
if [ "$1" = "--display" ] && [ "$2" = "--entitlements" ]; then
  cat "$INCODEX_CODESIGN_ENTITLEMENTS"
  exit 0
fi
if [ "$1" = "--display" ] && [ "$2" = "--verbose=4" ]; then
  case "$target" in
    *"Codex (Renderer).app") printf '%s\n' 'Identifier=com.openai.codex.renderer' 'TeamIdentifier=2DC432GLL2' 'Authority=Developer ID Application: OpenAI' ;;
    *"Codex Framework.framework")
      if [ -f "$INCODEX_FRAMEWORK_SIGN_STATE" ]; then
        printf '%s\n' 'Identifier=com.openai.codex.framework' 'Signature=adhoc'
      else
        printf '%s\n' 'Identifier=com.openai.codex.framework' 'TeamIdentifier=2DC432GLL2' 'Authority=Developer ID Application: OpenAI'
      fi
      ;;
    *RenamedVendor.xpc) {nested_display} ;;
    *Sparkle.framework) {nested_display} ;;
    *) {outer_display} ;;
  esac
  exit 0
fi
if [ "$1" = "--force" ] && [ "$2" = "--deep" ]; then
  printf '%s\n' signed > "$INCODEX_DEEP_SIGN_CAPTURE"
  marker="$target/Contents/Frameworks/RenamedVendor.xpc/Contents/vendor-marker"
  if [ -f "$marker" ]; then printf '%s\n' mutated-by-deep-sign > "$marker"; fi
  helper="$target/Contents/Frameworks/Codex Framework.framework/Versions/Current/Helpers/Codex (Renderer).app/Contents/vendor-marker"
  if [ -f "$helper" ]; then printf '%s\n' mutated-by-deep-sign > "$helper"; fi
  bare_helper="$target/Contents/Frameworks/Codex Framework.framework/Versions/Current/Helpers/browser_crashpad_handler"
  if [ -f "$bare_helper" ]; then printf '%s\n' mutated-by-deep-sign > "$bare_helper"; fi
  exit 0
fi
if [ "$1" = "--force" ] && [ "$2" = "--sign" ]; then
  printf '%s\n' "$target" >> "$INCODEX_SIGN_CAPTURE"
  case "$target" in
    *"Codex Framework.framework") printf '%s\n' signed > "$INCODEX_FRAMEWORK_SIGN_STATE" ;;
  esac
  exit 0
fi
if [ "$1" = "--verify" ] && [ "$2" = "--test-requirement" ]; then
  if [ "$INCODEX_CODESIGN_VENDOR_TRUST_FAILURE" = "1" ]; then exit 1; fi
  exit 0
fi
if [ "$1" = "--verify" ]; then
  case "$target" in
    *"Codex Framework.framework") [ -f "$INCODEX_FRAMEWORK_SIGN_STATE" ] ; exit $? ;;
  esac
  exit 0
fi
exit 0
"#
        );
        let codesign = fake_bin.join("codesign");
        fs::write(&codesign, script).unwrap();
        let mut permissions = fs::metadata(&codesign).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&codesign, permissions).unwrap();
        Self {
            root,
            app,
            sidecar,
            fake_bin,
            entitlements,
            marker,
            sparkle,
            codex_framework,
            keychain_provider,
            codex_helper_marker,
            codex_bare_helper_marker,
            framework_sign_state,
            sign_capture,
            deep_capture,
        }
    }

    fn install_keychain_provider_fixture(&self) {
        fs::create_dir_all(self.keychain_provider.parent().unwrap()).unwrap();
        fs::write(&self.keychain_provider, "patched-provider\n").unwrap();
        fs::create_dir_all(self.codex_helper_marker.parent().unwrap()).unwrap();
        fs::write(&self.codex_helper_marker, "official-helper\n").unwrap();
        fs::write(&self.codex_bare_helper_marker, "official-bare-helper\n").unwrap();
    }

    fn install_path(&self) -> OsString {
        let mut path = OsString::from(self.fake_bin.as_os_str());
        path.push(":");
        if let Some(existing) = std::env::var_os("PATH") {
            path.push(existing);
        }
        path
    }
}

#[test]
fn sparkle_update_framework_is_resigned_with_the_adhoc_host() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("2DC432GLL2");
    let original_path = std::env::var_os("PATH");
    let _path_guard = PathGuard(original_path);
    std::env::set_var("PATH", fixture.install_path());

    let preserved = collect_vendor_helper_roots(&fixture.app).unwrap();

    assert!(preserved.iter().any(|path| path == &fixture.sidecar));
    assert!(
        !preserved
            .iter()
            .any(|path| path.starts_with(&fixture.sparkle)),
        "Sparkle must share the ad-hoc host identity so its updater IPC accepts the patched app"
    );
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn run_sign(fixture: &Fixture) -> Result<(), String> {
    let original_path = std::env::var_os("PATH");
    let _path_guard = PathGuard(original_path);
    std::env::set_var("PATH", fixture.install_path());
    std::env::set_var("INCODEX_CODESIGN_ENTITLEMENTS", &fixture.entitlements);
    std::env::set_var("INCODEX_SIGN_CAPTURE", &fixture.sign_capture);
    std::env::set_var("INCODEX_DEEP_SIGN_CAPTURE", &fixture.deep_capture);
    std::env::set_var(
        "INCODEX_FRAMEWORK_SIGN_STATE",
        &fixture.framework_sign_state,
    );
    let result = sign_app(&fixture.app);
    std::env::remove_var("INCODEX_CODESIGN_ENTITLEMENTS");
    std::env::remove_var("INCODEX_SIGN_CAPTURE");
    std::env::remove_var("INCODEX_DEEP_SIGN_CAPTURE");
    std::env::remove_var("INCODEX_FRAMEWORK_SIGN_STATE");
    result
}

#[test]
fn renamed_vendor_component_is_preserved_by_signature_identity() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("2DC432GLL2");

    let result = run_sign(&fixture);

    assert!(
        result.is_ok(),
        "vendor sidecar should remain signable: {result:?}"
    );
    assert_eq!(
        fs::read_to_string(&fixture.marker).unwrap(),
        "original-vendor-component\n",
        "deep signing must not mutate a vendor sidecar, even when its filename is new"
    );
}

#[test]
fn unknown_signed_component_is_rejected_before_outer_signing() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("OTHERTEAM");

    let result = run_sign(&fixture);

    assert!(result.is_err(), "unknown signed sidecars must fail closed");
    assert!(
        !fixture.sign_capture.exists(),
        "outer ad-hoc signing must not run after rejecting a sidecar"
    );
    assert_eq!(
        fs::read_to_string(&fixture.marker).unwrap(),
        "original-vendor-component\n",
        "rejection must happen before any destructive signing step"
    );
}

#[test]
fn verified_generic_nested_component_can_be_resigned_with_third_party_outer() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new_custom_outer("OTHERTEAM");

    let result = run_sign(&fixture);

    assert!(
        result.is_ok(),
        "verified generic nested components should be signable: {result:?}"
    );
    assert!(fixture.sign_capture.exists());
}

#[test]
fn self_issued_vendor_lookalike_is_rejected_before_outer_signing() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("2DC432GLL2");
    let original_path = std::env::var_os("PATH");
    let _path_guard = PathGuard(original_path);
    std::env::set_var("PATH", fixture.install_path());
    std::env::set_var("INCODEX_CODESIGN_ENTITLEMENTS", &fixture.entitlements);
    std::env::set_var("INCODEX_SIGN_CAPTURE", &fixture.sign_capture);
    std::env::set_var("INCODEX_CODESIGN_VENDOR_TRUST_FAILURE", "1");

    let result = sign_app(&fixture.app);

    std::env::remove_var("INCODEX_CODESIGN_ENTITLEMENTS");
    std::env::remove_var("INCODEX_SIGN_CAPTURE");
    std::env::remove_var("INCODEX_CODESIGN_VENDOR_TRUST_FAILURE");
    assert!(
        result.is_err(),
        "self-issued vendor lookalikes must fail closed"
    );
    assert!(!fixture.sign_capture.exists());
    assert_eq!(
        fs::read_to_string(&fixture.marker).unwrap(),
        "original-vendor-component\n"
    );
}

#[test]
fn modified_codex_framework_joins_electron_helpers_to_host_identity() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("2DC432GLL2");
    fixture.install_keychain_provider_fixture();

    let result = run_sign(&fixture);

    assert!(
        result.is_ok(),
        "the provider-bearing Codex framework must be brought into the host ad-hoc signature generation: {result:?}"
    );
    assert_eq!(
        fs::read_to_string(&fixture.codex_helper_marker).unwrap(),
        "mutated-by-deep-sign\n",
        "the Renderer maps Codex Framework and must join its ad-hoc identity generation"
    );
    assert_eq!(
        fs::read_to_string(&fixture.codex_bare_helper_marker).unwrap(),
        "mutated-by-deep-sign\n",
        "framework helper executables must not retain a mismatched vendor Team ID"
    );
    assert_eq!(
        fs::read_to_string(&fixture.marker).unwrap(),
        "original-vendor-component\n",
        "external CUA/vendor sidecars must still retain their official identity"
    );
    let signed = fs::read_to_string(&fixture.sign_capture).unwrap();
    assert!(
        signed.contains(fixture.keychain_provider.to_string_lossy().as_ref()),
        "the patched provider dylib must receive a fresh linker signature"
    );
    assert_eq!(
        signed
            .lines()
            .filter(|line| *line == fixture.codex_framework.to_string_lossy())
            .count(),
        1,
        "the modified framework only needs the prerequisite seal before the deep host signing pass"
    );
}
