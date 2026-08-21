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
    display_count: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "incodex-signing-doctor-custom-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let app = root.join("ThirdParty.app");
        let nested = app.join("Contents/Frameworks/NestedVendor.xpc");
        let fake_bin = root.join("fake-bin");
        let display_count = root.join("display-count");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::create_dir_all(nested.join("Contents/_CodeSignature")).unwrap();
        fs::create_dir_all(&fake_bin).unwrap();
        fs::write(app.join("Contents/MacOS/ThirdParty"), "binary\n").unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.example.third-party</string>
<key>CFBundleShortVersionString</key><string>1.0.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleExecutable</key><string>ThirdParty</string>
</dict></plist>
"#,
        )
        .unwrap();
        let script = r#"#!/bin/sh
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
  count="$(cat "$INCODEX_CODESIGN_DISPLAY_COUNT" 2>/dev/null || echo 0)"
  echo $((count + 1)) > "$INCODEX_CODESIGN_DISPLAY_COUNT"
  case "$target" in
    *NestedVendor.xpc) printf '%s\n' 'Identifier=com.example.vendor' 'TeamIdentifier=2DC432GLL2' 'Authority=Developer ID Application: fixture' ;;
    *) printf '%s\n' 'Identifier=com.example.third-party' 'TeamIdentifier=THIRDPARTY' 'Authority=Developer ID Application: third-party fixture' ;;
  esac
  exit 0
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
            display_count,
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
fn doctor_uses_generic_acceptance_for_an_unpatched_custom_bundle() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new();
    let original_path = std::env::var_os("PATH");
    let _path_guard = PathGuard(original_path);
    std::env::set_var("PATH", fixture.path());
    std::env::set_var("INCODEX_CODESIGN_DISPLAY_COUNT", &fixture.display_count);

    let report = serde_json::to_value(diagnose_with_root(&fixture.app, &fixture.root)).unwrap();
    let display_count = fs::read_to_string(&fixture.display_count)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    std::env::remove_var("INCODEX_CODESIGN_DISPLAY_COUNT");
    let signing = &report["signing"];
    assert_eq!(signing["verified"], true);
    assert_eq!(signing["outer"]["kind"], "other");
    assert_eq!(display_count, 2, "Doctor should inspect outer and nested components once");
    assert!(!report["checks"]["signing"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "signing.outer-identity"));
}
