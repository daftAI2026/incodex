use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use incodex_asar::pack_dir;
use sha2::{Digest, Sha256};

pub(super) fn marker_app(home: &Path) -> PathBuf {
    let app = home.join("Marker.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("marker"), "do-not-touch\n").unwrap();
    app
}

pub(super) fn write_executable(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

pub(super) fn patchable_app(home: &Path) -> PathBuf {
    let root = home.join("bundle");
    let app = root.join("ChatGPT.app");
    let contents = app.join("Contents");
    fs::create_dir_all(contents.join("Resources")).unwrap();
    fs::write(
        contents.join("Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>com.example.incodex-fixture</string>
  <key>CFBundleName</key>
  <string>ChatGPT</string>
  <key>CFBundleShortVersionString</key>
  <string>1.2.3</string>
  <key>CFBundleVersion</key>
  <string>123</string>
  <key>CFBundleExecutable</key>
  <string>ChatGPT</string>
</dict>
</plist>
"#,
    )
    .unwrap();
    write_executable(
        &contents.join("MacOS").join("ChatGPT"),
        "#!/bin/sh\nexit 0\n",
    );
    let cua_app = contents.join("Frameworks/CUALockScreenGuardian.app");
    fs::create_dir_all(cua_app.join("Contents")).unwrap();
    fs::write(
        cua_app.join("Contents/Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.example.cua-guardian</string>
  <key>CFBundleExecutable</key><string>CUALockScreenGuardian</string>
  <key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#,
    )
    .unwrap();
    write_executable(
        &cua_app.join("Contents/MacOS/CUALockScreenGuardian"),
        "#!/bin/sh\necho vendor-helper\nexit 0\n",
    );
    let signed = Command::new("codesign")
        .args(["--force", "--sign", "-", "--"])
        .arg(&cua_app)
        .status()
        .expect("sign fixture vendor helper");
    assert!(signed.success());
    let src = home.join("asar-src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("package.json"),
        format!("{}\n", serde_json::json!({"main":"index.js"})),
    )
    .unwrap();
    fs::write(src.join("index.js"), "ok\n").unwrap();
    pack_dir(&src, &contents.join("Resources").join("app.asar")).unwrap();
    app
}

pub(super) fn codesign_display(path: &Path) -> String {
    let output = Command::new("codesign")
        .args(["-d", "-v", "--"])
        .arg(path)
        .output()
        .expect("codesign");
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

pub(super) fn is_signed(path: &Path) -> bool {
    Command::new("codesign")
        .args(["--verify", "--"])
        .arg(path)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(super) fn tree_digest(root: &Path) -> String {
    let mut entries = Vec::new();
    collect_tree_entries(root, root, &mut entries);
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let mut digest = Sha256::new();
    for (relative, bytes) in entries {
        digest.update(relative.as_bytes());
        digest.update([0]);
        digest.update(bytes);
        digest.update([0]);
    }
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn collect_tree_entries(root: &Path, current: &Path, entries: &mut Vec<(String, Vec<u8>)>) {
    for entry in fs::read_dir(current).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let file_type = entry.file_type().unwrap();
        let relative = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if file_type.is_symlink() {
            entries.push((
                relative,
                fs::read_link(&path)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
                    .into_bytes(),
            ));
        } else if file_type.is_dir() {
            collect_tree_entries(root, &path, entries);
        } else {
            entries.push((relative, fs::read(path).unwrap()));
        }
    }
}
