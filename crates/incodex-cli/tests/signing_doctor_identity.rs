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
}

impl Fixture {
    fn new(identity: &'static str) -> Self {
        Self::with_outer(identity, false)
    }

    fn new_with_invalid_outer(identity: &'static str) -> Self {
        Self::with_outer(identity, true)
    }

    fn with_outer(identity: &'static str, invalid_outer: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "incodex-signing-doctor-identity-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let app = root.join("ChatGPT.app");
        let nested = app.join("Contents/Frameworks/Nested.xpc");
        let fake_bin = root.join("fake-bin");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::create_dir_all(nested.join("Contents/_CodeSignature")).unwrap();
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
        let outer_display = if invalid_outer {
            "printf '%s\\n' 'Identifier=com.example.third-party' 'TeamIdentifier=THIRDPARTY'"
        } else {
            "printf '%s\\n' 'Identifier=com.openai.codex' 'TeamIdentifier=2DC432GLL2' 'Authority=Developer ID Application: fixture'"
        };
        let script = format!(
            r#"#!/bin/sh
target=""
for arg in "$@"; do target="$arg"; done
if [ "$1" = "--display" ] && [ "$2" = "--entitlements" ]; then
  printf '%s' '<?xml version="1.0"?><plist><dict></dict></plist>'
  exit 0
fi
if [ "$1" = "--display" ] && [ "$2" = "--verbose=2" ]; then
  printf '%s\n' 'flags=0x10000(runtime)'
  exit 0
fi
if [ "$1" = "--display" ] && [ "$2" = "--verbose=4" ]; then
  case "$target" in
    *Nested.xpc) {identity_display} ;;
    *) {outer_display} ;;
  esac
  exit 0
fi
if [ "$1" = "--verify" ]; then exit 0; fi
exit 0
"#,
            identity_display = match identity {
                "other" => "printf '%s\\n' 'Identifier=com.example.other' 'TeamIdentifier=OTHERTEAM'",
                "valid-other" => "printf '%s\\n' 'Identifier=com.example.other' 'TeamIdentifier=OTHERTEAM' 'Authority=Other Signer'",
                "unknown" => "printf '%s\\n' 'Identifier=com.example.unknown'",
                _ => unreachable!(),
            },
            outer_display = outer_display,
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
        }
    }

    fn path(&self) -> OsString {
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
fn doctor_does_not_report_other_or_unknown_nested_identity_as_clean() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    for identity in ["other", "unknown"] {
        let fixture = Fixture::new(identity);
        let original_path = std::env::var_os("PATH");
        let _path_guard = PathGuard(original_path);
        std::env::set_var("PATH", fixture.path());

        let report = serde_json::to_value(diagnose_with_root(&fixture.app, &fixture.root)).unwrap();
        let signing = &report["signing"];
        assert_eq!(signing["verified"], false, "identity={identity}");
        assert_eq!(report["checks"]["signing"]["status"], "checked");
        assert!(report["checks"]["signing"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == "signing.component-identity-unsupported"), "identity={identity}");
        std::env::remove_var("PATH");
    }
}

#[test]
fn doctor_generic_other_component_does_not_hide_outer_acceptance_failure() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new_with_invalid_outer("valid-other");
    let original_path = std::env::var_os("PATH");
    let _path_guard = PathGuard(original_path);
    std::env::set_var("PATH", fixture.path());

    let report = serde_json::to_value(diagnose_with_root(&fixture.app, &fixture.root)).unwrap();
    let findings = report["checks"]["signing"]["findings"].as_array().unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding["code"] == "signing.acceptance-failed"));
}
