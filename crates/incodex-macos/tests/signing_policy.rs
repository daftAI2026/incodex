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
    capture: PathBuf,
}

impl Fixture {
    fn new(entitlements: &str, display_succeeds: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "incodex-signing-policy-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let app = root.join("ChatGPT.app");
        let fake_bin = root.join("fake-bin");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
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
        fs::write(root.join("host-entitlements.plist"), entitlements).unwrap();
        let capture = root.join("captured-entitlements.plist");
        let display_status = if display_succeeds { "0" } else { "1" };
        let script = format!(
            "#!/bin/sh\nif [ \"$1\" = \"--display\" ] && [ \"$2\" = \"--entitlements\" ]; then\n  cat \"$INCODEX_CODESIGN_ENTITLEMENTS\"\n  exit {display_status}\nfi\nif [ \"$1\" = \"--display\" ] && [ \"$2\" = \"--verbose=4\" ]; then\n  printf '%s\\n' 'Identifier=com.example.fixture' 'Signature=adhoc'\n  exit 0\nfi\nprevious=\"\"\nfor arg in \"$@\"; do\n  if [ \"$previous\" = \"--entitlements\" ] && [ \"$arg\" != \":-\" ]; then\n    cat \"$arg\" > \"$INCODEX_CODESIGN_CAPTURE\"\n  fi\n  previous=\"$arg\"\ndone\nexit 0\n"
        );
        let codesign = fake_bin.join("codesign");
        fs::write(&codesign, script).unwrap();
        let mut permissions = fs::metadata(&codesign).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&codesign, permissions).unwrap();
        let entitlements_path = root.join("host-entitlements.plist");
        Self {
            root,
            app,
            fake_bin,
            entitlements: entitlements_path,
            capture,
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
fn entitlement_inspection_failure_fails_closed_before_signing() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new(
        r#"<?xml version="1.0"?><plist><dict><key>com.apple.security.device.camera</key><true/></dict></plist>"#,
        false,
    );
    let original_path = std::env::var_os("PATH");
    let _path_guard = PathGuard(original_path);
    std::env::set_var("PATH", fixture.install_path());
    std::env::set_var("INCODEX_CODESIGN_ENTITLEMENTS", &fixture.entitlements);
    std::env::set_var("INCODEX_CODESIGN_CAPTURE", &fixture.capture);

    let result = sign_app(&fixture.app);

    std::env::remove_var("INCODEX_CODESIGN_ENTITLEMENTS");
    std::env::remove_var("INCODEX_CODESIGN_CAPTURE");
    assert!(result.is_err(), "entitlement inspection must fail closed");
    assert!(
        !fixture.capture.exists(),
        "codesign must not run with a guessed fallback entitlement set"
    );
}

#[test]
fn missing_camera_does_not_trigger_broad_entitlement_fallback() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new(
        r#"<?xml version="1.0"?><plist><dict><key>com.apple.security.cs.allow-jit</key><true/></dict></plist>"#,
        true,
    );
    let original_path = std::env::var_os("PATH");
    let _path_guard = PathGuard(original_path);
    std::env::set_var("PATH", fixture.install_path());
    std::env::set_var("INCODEX_CODESIGN_ENTITLEMENTS", &fixture.entitlements);
    std::env::set_var("INCODEX_CODESIGN_CAPTURE", &fixture.capture);

    let result = sign_app(&fixture.app);

    std::env::remove_var("INCODEX_CODESIGN_ENTITLEMENTS");
    std::env::remove_var("INCODEX_CODESIGN_CAPTURE");
    assert!(
        result.is_ok(),
        "source entitlements should remain signable: {result:?}"
    );
    let captured = fs::read_to_string(&fixture.capture).unwrap();
    assert!(captured.contains("com.apple.security.cs.allow-jit"));
    assert!(captured.contains("com.apple.security.cs.disable-library-validation"));
    for broad in [
        "com.apple.security.device.camera",
        "com.apple.security.device.audio-input",
        "com.apple.security.personal-information.calendars",
        "com.apple.security.network.client",
    ] {
        assert!(
            !captured.contains(broad),
            "unexpected broad entitlement: {broad}"
        );
    }
}
