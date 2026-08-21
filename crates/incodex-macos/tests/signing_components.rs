use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_macos::sign_app;

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
    fake_bin: PathBuf,
    entitlements: PathBuf,
    marker: PathBuf,
    sign_capture: PathBuf,
}

impl Fixture {
    fn new(identity: &str) -> Self {
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
        let fake_bin = root.join("fake-bin");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::create_dir_all(sidecar.join("Contents/_CodeSignature")).unwrap();
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
    *RenamedVendor.xpc) printf '%s\n' 'Identifier=com.example.renamed-vendor' 'TeamIdentifier={identity}' 'Authority=Developer ID Application: fixture' ;;
    *) printf '%s\n' 'Identifier=com.openai.codex' 'Signature=adhoc' ;;
  esac
  exit 0
fi
if [ "$1" = "--force" ] && [ "$2" = "--deep" ]; then
  marker="$target/Contents/Frameworks/RenamedVendor.xpc/Contents/vendor-marker"
  if [ -f "$marker" ]; then printf '%s\n' mutated-by-deep-sign > "$marker"; fi
  exit 0
fi
if [ "$1" = "--force" ] && [ "$2" = "--sign" ]; then
  printf '%s\n' signed >> "$INCODEX_SIGN_CAPTURE"
  exit 0
fi
if [ "$1" = "--verify" ] && [ "$2" = "--test-requirement" ]; then
  if [ "$INCODEX_CODESIGN_VENDOR_TRUST_FAILURE" = "1" ]; then exit 1; fi
  exit 0
fi
if [ "$1" = "--verify" ]; then exit 0; fi
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
            fake_bin,
            entitlements,
            marker,
            sign_capture,
        }
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
    let result = sign_app(&fixture.app);
    std::env::remove_var("INCODEX_CODESIGN_ENTITLEMENTS");
    std::env::remove_var("INCODEX_SIGN_CAPTURE");
    result
}

#[test]
fn renamed_vendor_component_is_preserved_by_signature_identity() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("2DC432GLL2");

    let result = run_sign(&fixture);

    assert!(result.is_ok(), "vendor sidecar should remain signable: {result:?}");
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
    assert!(result.is_err(), "self-issued vendor lookalikes must fail closed");
    assert!(!fixture.sign_capture.exists());
    assert_eq!(
        fs::read_to_string(&fixture.marker).unwrap(),
        "original-vendor-component\n"
    );
}
