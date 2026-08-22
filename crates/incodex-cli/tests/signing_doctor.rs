use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_cli::diagnose::diagnose_with_root;

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
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "incodex-signing-doctor-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let app = root.join("ChatGPT.app");
        let nested = app.join("Contents/Frameworks/Damaged.xpc");
        let fake_bin = root.join("fake-bin");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::create_dir_all(&nested).unwrap();
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
        fs::create_dir_all(nested.join("Contents/_CodeSignature")).unwrap();
        let entitlements = root.join("entitlements.plist");
        fs::write(
            &entitlements,
            r#"<?xml version="1.0"?><plist><dict>
<key>com.apple.application-identifier</key><string>2DC432GLL2.com.openai.codex</string>
<key>com.apple.security.cs.allow-jit</key><true/>
</dict></plist>"#,
        )
        .unwrap();
        let script = r#"#!/bin/sh
target=""
for arg in "$@"; do target="$arg"; done
if [ "$1" = "--display" ] && [ "$2" = "--entitlements" ]; then
  cat "$INCODEX_CODESIGN_ENTITLEMENTS"
  exit 0
fi
if [ "$1" = "--display" ] && [ "$2" = "--verbose=2" ]; then
  printf '%s\n' 'flags=0x10000(runtime)'
  exit 0
fi
if [ "$1" = "--display" ] && [ "$2" = "--verbose=4" ]; then
  printf '%s\n' 'Identifier=com.openai.codex' 'TeamIdentifier=2DC432GLL2' 'Authority=Developer ID Application: fixture'
  exit 0
fi
if [ "$1" = "--verify" ] && printf '%s' "$target" | grep -q 'Damaged.xpc'; then
  printf '%s\n' 'nested signature is damaged' >&2
  exit 1
fi
if [ "$1" = "--verify" ]; then exit 0; fi
exit 0
"#;
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

#[test]
fn doctor_reports_nested_signature_damage_and_unretainable_entitlements() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new();
    let original_path = std::env::var_os("PATH");
    let _path_guard = PathGuard(original_path);
    std::env::set_var("PATH", fixture.install_path());
    std::env::set_var("INCODEX_CODESIGN_ENTITLEMENTS", &fixture.entitlements);

    let report = diagnose_with_root(&fixture.app, &fixture.root);
    std::env::remove_var("INCODEX_CODESIGN_ENTITLEMENTS");
    let json = serde_json::to_value(report).unwrap();
    let signing = &json["signing"];

    assert_eq!(signing["status"], "checked");
    assert_eq!(signing["componentCount"], 1);
    assert_eq!(
        signing["unretainable"],
        serde_json::json!(["com.apple.application-identifier"])
    );
    assert_eq!(signing["components"][0]["verified"], false);
    assert_eq!(json["checks"]["signing"]["status"], "checked");
    assert!(json["checks"]["signing"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "signing.component-invalid"));
}
