//! codesign, plist, ditto, Launch Services, and quit.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const VENDOR_HELPER_NAMES: &[&str] = &[
    "Codex Computer Use.app",
    "Codex Computer Use Installer.app",
    "SkyComputerUseClient.app",
    "CUALockScreenGuardian.app",
];

#[derive(Debug, Clone, Default)]
pub struct PlistInfo {
    pub bundle_identifier: String,
    pub app_version: String,
    pub app_build: String,
    pub executable: String,
}

pub fn ditto(src: &Path, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        fs::remove_dir_all(dest).map_err(|err| err.to_string())?;
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let output = Command::new("ditto")
        .args([src, dest])
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

pub fn restore_original(source: &Path, dest: &Path) -> Result<(), String> {
    if !source.exists() {
        return Err(format!("original snapshot missing: {}", source.display()));
    }
    let staged = PathBuf::from(format!("{}.incodex-restore", dest.display()));
    ditto(source, &staged)?;
    let trash = PathBuf::from(format!("{}.incodex-uninstall", dest.display()));
    if trash.exists() {
        fs::remove_dir_all(&trash).map_err(|err| err.to_string())?;
    }
    if dest.exists() {
        fs::rename(dest, &trash).map_err(|err| err.to_string())?;
    }
    if let Err(err) = fs::rename(&staged, dest) {
        if trash.exists() {
            let _ = fs::rename(&trash, dest);
        }
        return Err(err.to_string());
    }
    if trash.exists() {
        fs::remove_dir_all(&trash).map_err(|err| err.to_string())?;
    }
    Ok(())
}

pub fn read_plist_info(app: &Path) -> Option<PlistInfo> {
    let plist = app.join("Contents").join("Info.plist");
    if !plist.exists() {
        return None;
    }
    let output = Command::new("plutil")
        .args(["-convert", "json", "-o", "-", "--"])
        .arg(&plist)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    Some(PlistInfo {
        bundle_identifier: json_string(&raw, "CFBundleIdentifier"),
        app_version: json_string(&raw, "CFBundleShortVersionString"),
        app_build: json_string(&raw, "CFBundleVersion"),
        executable: raw
            .get("CFBundleExecutable")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("ChatGPT")
            .to_string(),
    })
}

/// 读取 `CFBundleExecutable` 的严格版本，供启动路径使用。
pub fn read_plist_executable(app: &Path) -> Result<String, String> {
    let plist = app.join("Contents").join("Info.plist");
    if !plist.is_file() {
        return Err(format!("Info.plist not found: {}", plist.display()));
    }
    let output = Command::new("plutil")
        .args(["-convert", "json", "-o", "-", "--"])
        .arg(&plist)
        .output()
        .map_err(|error| format!("cannot read {}: {error}", plist.display()))?;
    if !output.status.success() {
        return Err(format!("Info.plist is invalid: {}", plist.display()));
    }
    let raw: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Info.plist is not valid JSON: {error}"))?;
    raw.get("CFBundleExecutable")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "Info.plist has no valid CFBundleExecutable: {}",
                plist.display()
            )
        })
}

pub fn read_architecture(app: &Path, executable: &str) -> Option<String> {
    let binary = app.join("Contents").join("MacOS").join(executable);
    let output = Command::new("lipo")
        .arg("-archs")
        .arg(binary)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut architectures: Vec<_> = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(str::to_string)
        .collect();
    architectures.sort();
    let joined = architectures.join(" ");
    (!joined.is_empty()).then_some(joined)
}

pub fn read_asar_integrity(app: &Path) -> Option<String> {
    let plist = app.join("Contents").join("Info.plist");
    let output = Command::new("plutil")
        .args(["-convert", "json", "-o", "-", "--"])
        .arg(plist)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    raw.pointer("/ElectronAsarIntegrity/Resources~1app.asar/hash")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn diagnose_spctl(app: &Path) -> serde_json::Value {
    let output = Command::new("spctl")
        .args(["--assess", "--verbose=4", "--"])
        .arg(app)
        .output();
    match output {
        Ok(output) => {
            let status = output.status.code().unwrap_or(1);
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .trim()
            .to_string();
            serde_json::json!({
                "status": status,
                "output": text,
                "accepted": output.status.success(),
                "usedAsSuccessGate": false,
            })
        }
        Err(error) => serde_json::json!({
            "status": 1,
            "output": error.to_string(),
            "accepted": false,
            "usedAsSuccessGate": false,
        }),
    }
}

fn json_string(raw: &serde_json::Value, key: &str) -> String {
    match raw.get(key) {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

pub fn write_asar_integrity(app: &Path, hash: &str) -> Result<(), String> {
    let plist = app.join("Contents").join("Info.plist");
    if !plist.exists() {
        return Ok(());
    }
    let payload = serde_json::json!({
        "Resources/app.asar": { "algorithm": "SHA256", "hash": hash }
    });
    let json = serde_json::to_string(&payload).map_err(|err| err.to_string())?;
    let mut failures = Vec::new();
    for flag in ["-replace", "-insert"] {
        let result = Command::new("plutil")
            .args([flag, "ElectronAsarIntegrity", "-json", &json])
            .arg(&plist)
            .status();
        match result {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => failures.push(format!("plutil {flag} exited with {status}")),
            Err(err) => failures.push(format!("plutil {flag} failed: {err}")),
        }
    }
    Err(format!(
        "failed to update ElectronAsarIntegrity in {}: {}",
        plist.display(),
        failures.join("; ")
    ))
}

pub fn verify_app(app: &Path) -> bool {
    Command::new("codesign")
        .args(["--verify", "--"])
        .arg(app)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn has_hardened_runtime(app: &Path) -> bool {
    let Ok(output) = Command::new("codesign")
        .args(["--display", "--verbose=2", "--"])
        .arg(app)
        .output()
    else {
        return false;
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    text.lines()
        .filter_map(|line| line.split_once("flags=").map(|(_, flags)| flags))
        .any(|flags| flags.contains("runtime"))
}

const ADHOC_UNRETAINABLE_ENTITLEMENTS: &[&str] = &[
    "com.apple.developer.team-identifier",
    "com.apple.application-identifier",
    "com.apple.developer.aps-environment",
    "com.apple.security.application-groups",
    "keychain-access-groups",
];

const ADHOC_HOST_FALLBACK: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>com.apple.security.automation.apple-events</key><true/>
  <key>com.apple.security.cs.allow-jit</key><true/>
  <key>com.apple.security.cs.allow-unsigned-executable-memory</key><true/>
  <key>com.apple.security.cs.disable-library-validation</key><true/>
  <key>com.apple.security.device.audio-input</key><true/>
  <key>com.apple.security.device.camera</key><true/>
  <key>com.apple.security.files.user-selected.read-write</key><true/>
  <key>com.apple.security.network.client</key><true/>
  <key>com.apple.security.personal-information.calendars</key><true/>
</dict></plist>
"#;

const DISABLE_LIBRARY_VALIDATION: &str =
    "  <key>com.apple.security.cs.disable-library-validation</key><true/>\n";

fn dump_entitlements(target: &Path) -> Option<String> {
    let output = Command::new("codesign")
        .args(["--display", "--entitlements", ":-", "--"])
        .arg(target)
        .output()
        .ok()?;
    let xml = String::from_utf8_lossy(&output.stdout).into_owned();
    xml.contains("<plist").then_some(xml)
}

fn strip_unretainable_entitlements(xml: Option<&str>) -> Option<String> {
    let mut next = xml?.to_string();
    for key in ADHOC_UNRETAINABLE_ENTITLEMENTS {
        let marker = format!("<key>{key}</key>");
        while let Some(start) = next.find(&marker) {
            let value_start = start + marker.len();
            let value_start = value_start
                + next[value_start..]
                    .chars()
                    .take_while(|ch| ch.is_whitespace())
                    .map(char::len_utf8)
                    .sum::<usize>();
            let value_end = xml_value_end(&next, value_start)?;
            next.replace_range(start..value_end, "");
        }
    }
    next.contains("<key>").then_some(next)
}

fn xml_value_end(xml: &str, start: usize) -> Option<usize> {
    let rest = xml.get(start..)?;
    if rest.starts_with("<true/>") || rest.starts_with("<false/>") {
        return Some(start + rest.find('>')? + 1);
    }
    let open_end = rest.find('>')?;
    let open = &rest[1..open_end];
    let name = open
        .split_whitespace()
        .next()?
        .trim_end_matches('/')
        .to_string();
    let close = format!("</{name}>");
    let close_start = rest[open_end + 1..].find(&close)? + open_end + 1;
    Some(start + close_start + close.len())
}

fn with_disable_library_validation(xml: Option<&str>) -> Option<String> {
    let base = xml.unwrap_or("<?xml version=\"1.0\"?><plist><dict></dict></plist>");
    if base.contains("<key>com.apple.security.cs.disable-library-validation</key>") {
        return Some(base.to_string());
    }
    base.contains("</dict>").then(|| {
        base.replacen(
            "</dict>",
            &format!("{DISABLE_LIBRARY_VALIDATION}</dict>"),
            1,
        )
    })
}

fn host_entitlements_for_adhoc(xml: Option<&str>) -> String {
    let stripped = strip_unretainable_entitlements(xml);
    if stripped
        .as_deref()
        .map(|value| !value.contains("<key>com.apple.security.device.camera</key>"))
        .unwrap_or(true)
    {
        return ADHOC_HOST_FALLBACK.to_string();
    }
    with_disable_library_validation(stripped.as_deref())
        .unwrap_or_else(|| ADHOC_HOST_FALLBACK.to_string())
}

fn temporary_entitlements_file(xml: &str) -> Result<(PathBuf, PathBuf), String> {
    let root = std::env::temp_dir().join(format!(
        "incodex-ent-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&root).map_err(|err| err.to_string())?;
    let file = root.join("entitlements.plist");
    if let Err(err) = fs::write(&file, xml) {
        let _ = fs::remove_dir_all(&root);
        return Err(err.to_string());
    }
    Ok((root, file))
}

fn sign_outer_with_entitlements(app: &Path, entitlements: &str) -> Result<(), String> {
    let (root, file) = temporary_entitlements_file(entitlements)?;
    let result = Command::new("codesign")
        .args([
            "--force",
            "--sign",
            "-",
            "--options",
            "runtime",
            "--entitlements",
        ])
        .arg(&file)
        .args(["--"])
        .arg(app)
        .output()
        .map_err(|err| err.to_string())
        .and_then(|output| {
            if output.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
            }
        });
    let cleanup = fs::remove_dir_all(root).map_err(|err| err.to_string());
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(err), Ok(())) => Err(err),
        (Ok(()), Err(err)) => Err(err),
        (Err(sign), Err(cleanup)) => {
            Err(format!("{sign}; failed to clean entitlements: {cleanup}"))
        }
    }
}

pub fn collect_vendor_helper_roots(app: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk_helpers(app, &mut found);
    let mut roots = Vec::new();
    for path in &found {
        if !found
            .iter()
            .any(|other| other != path && path.starts_with(other))
        {
            roots.push(path.clone());
        }
    }
    roots
}

fn walk_helpers(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if VENDOR_HELPER_NAMES.contains(&name.as_ref()) {
            out.push(path);
            continue;
        }
        if path.is_dir() {
            walk_helpers(&path, out);
        }
    }
}

pub fn sign_app(app: &Path) -> Result<(), String> {
    let before_xml = dump_entitlements(app);
    let preserve = collect_vendor_helper_roots(app);
    let stash_root = if preserve.is_empty() {
        None
    } else {
        let dir = std::env::temp_dir().join(format!(
            "incodex-vendor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
        Some(dir)
    };
    let mut stashed = Vec::new();
    if let Some(root) = &stash_root {
        for (index, src) in preserve.iter().enumerate() {
            let dest = root.join(index.to_string()).join(
                src.file_name()
                    .ok_or_else(|| "vendor helper missing name".to_string())?,
            );
            ditto(src, &dest)?;
            fs::remove_dir_all(src).map_err(|err| err.to_string())?;
            stashed.push((src.clone(), dest));
        }
    }
    let deep = Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-", "--"])
        .arg(app)
        .output()
        .map_err(|err| err.to_string())
        .and_then(|output| {
            if output.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
            }
        });
    let restore = restore_stashed_helpers(&stashed, stash_root.as_deref());
    if let Err(err) = restore {
        return Err(match deep {
            Ok(()) => err,
            Err(deep_err) => format!("{deep_err}; {err}"),
        });
    }
    deep?;
    sign_outer_with_entitlements(app, &host_entitlements_for_adhoc(before_xml.as_deref()))?;
    if !verify_app(app) {
        return Err("codesign --verify failed after adhoc resign".into());
    }
    Ok(())
}

fn restore_stashed_helpers(
    stashed: &[(PathBuf, PathBuf)],
    stash_root: Option<&Path>,
) -> Result<(), String> {
    let mut failures = Vec::new();
    for (src, dest) in stashed {
        if let Err(err) = ditto(dest, src) {
            failures.push(format!("failed to restore {}: {err}", src.display()));
        }
    }
    if failures.is_empty() {
        if let Some(root) = stash_root {
            if let Err(err) = fs::remove_dir_all(root) {
                failures.push(format!(
                    "failed to remove vendor stash {}: {err}",
                    root.display()
                ));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

pub fn quit_official_app() -> Result<(), String> {
    let script = r#"tell application id "com.openai.codex" to quit"#;
    let output = Command::new("osascript")
        .args(["-e", script])
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

pub fn front_codex_window_bounds() -> Option<(i32, i32, i32, i32)> {
    let script = r#"tell application "System Events" to tell first process whose bundle identifier is "com.openai.codex" to get {position, size} of front window"#;
    let output = Command::new("osascript")
        .args(["-e", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_window_bounds_output(&String::from_utf8_lossy(&output.stdout))
}

pub fn tile_process_front_window(
    pid: u32,
    source: (i32, i32, i32, i32),
    offset: i32,
) -> Result<(), String> {
    let desired = (source.0 + offset, source.1 + offset, source.2, source.3);
    set_process_front_window_bounds(pid, desired)?;
    let actual = process_front_window_bounds(pid).ok_or("child window bounds unavailable")?;
    if actual.2 != source.2 || actual.3 != source.3 {
        set_process_front_window_bounds(pid, source)?;
    }
    Ok(())
}

fn process_front_window_bounds(pid: u32) -> Option<(i32, i32, i32, i32)> {
    let script = format!(
        "tell application \"System Events\" to tell first process whose unix id is {pid} to get {{position, size}} of front window"
    );
    let output = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_window_bounds_output(&String::from_utf8_lossy(&output.stdout))
}

fn set_process_front_window_bounds(pid: u32, bounds: (i32, i32, i32, i32)) -> Result<(), String> {
    let script = format!(
        "tell application \"System Events\" to tell first process whose unix id is {pid} to tell front window to set {{position, size}} to {{{{{}, {}}}, {{{}, {}}}}}",
        bounds.0, bounds.1, bounds.2, bounds.3
    );
    let output = Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(())
}

fn parse_window_bounds_output(raw: &str) -> Option<(i32, i32, i32, i32)> {
    let values: Vec<i32> = raw
        .trim()
        .split(',')
        .map(str::trim)
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    (values.len() == 4).then(|| (values[0], values[1], values[2], values[3]))
}

pub fn notify_launch_services(app: &Path) -> Result<(), String> {
    let _ = Command::new("lsregister")
        .args(["-f", "-R", "-trusted"])
        .arg(app)
        .output();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

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

    #[test]
    fn crate_compiles() {}

    #[test]
    fn parses_system_events_position_and_size() {
        assert_eq!(
            parse_window_bounds_output("0, 34, 1710, 1073\n"),
            Some((0, 34, 1710, 1073))
        );
        assert_eq!(parse_window_bounds_output("missing"), None);
    }

    #[test]
    fn notify_launch_services_captures_tool_output() {
        const CHILD: &str = "INCODEX_NOTIFY_LS_CHILD";
        if std::env::var_os(CHILD).is_some() {
            notify_launch_services(Path::new("/tmp/ChatGPT.app")).unwrap();
            return;
        }

        let _path_lock = PATH_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "incodex-lsregister-output-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let fake = root.join("lsregister");
        std::fs::write(
            &fake,
            "#!/bin/sh\nprintf '%s\\n' LSREGISTER-OUT\nprintf '%s\\n' LSREGISTER-ERR >&2\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&fake).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake, permissions).unwrap();

        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::notify_launch_services_captures_tool_output",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("PATH", &root)
            .output()
            .unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stdout.contains("LSREGISTER-OUT"), "{stdout:?}");
        assert!(!stderr.contains("LSREGISTER-ERR"), "{stderr:?}");
    }

    #[test]
    fn hardened_runtime_requires_runtime_in_codesign_flags() {
        let _path_lock = PATH_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "incodex-codesign-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let fake_codesign = root.join("codesign");
        std::fs::write(
            &fake_codesign,
            "#!/bin/sh\nprintf '%s\\n' 'Executable=/tmp/runtime-target.app/Contents/MacOS/runtime-target' 'Identifier=com.example.runtime-target' 'flags=0x0(none)'\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&fake_codesign).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_codesign, permissions).unwrap();

        let original_path = std::env::var_os("PATH");
        let _path_guard = PathGuard(original_path.clone());
        let mut path = OsString::from(root.as_os_str());
        path.push(":");
        if let Some(original_path) = original_path {
            path.push(original_path);
        }
        std::env::set_var("PATH", path);

        let hardened = has_hardened_runtime(Path::new("/tmp/runtime-target.app"));
        std::fs::remove_dir_all(&root).unwrap();

        assert!(!hardened);
    }

    #[test]
    fn adhoc_outer_signature_uses_filtered_host_entitlements() {
        let _path_lock = PATH_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "incodex-host-entitlements-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let app = root.join("Mini.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(app.join("Contents/Info.plist"), "fixture").unwrap();
        let host = root.join("host-entitlements.plist");
        std::fs::write(
            &host,
            r#"<?xml version="1.0"?><plist><dict>
  <key>com.apple.developer.team-identifier</key><string>ABCD123456</string>
  <key>com.apple.application-identifier</key><string>ABCD.com.incodex.mini</string>
  <key>keychain-access-groups</key><array><string>ABCD.com.incodex.mini</string></array>
  <key>com.apple.security.cs.allow-jit</key><true/>
  <key>com.apple.security.device.camera</key><true/>
</dict></plist>"#,
        )
        .unwrap();
        let fake_codesign = root.join("codesign");
        std::fs::write(
            &fake_codesign,
            r##"#!/bin/sh
printf '%s\n' "$*" >> "$INCODEX_CODESIGN_LOG"
if [ "$1" = "--display" ] && [ "$2" = "--entitlements" ]; then
  cat "$INCODEX_CODESIGN_ENTITLEMENTS"
  exit 0
fi
previous=""
for arg in "$@"; do
  if [ "$previous" = "--entitlements" ] && [ "$arg" != ":-" ]; then
    cat "$arg" > "$INCODEX_CODESIGN_CAPTURE"
  fi
  previous="$arg"
done
exit 0
"##,
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&fake_codesign).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_codesign, permissions).unwrap();

        let original_path = std::env::var_os("PATH");
        let _path_guard = PathGuard(original_path.clone());
        let mut path = OsString::from(root.as_os_str());
        path.push(":");
        if let Some(original_path) = original_path {
            path.push(original_path);
        }
        let log = root.join("codesign.log");
        let capture = root.join("captured-entitlements.plist");
        std::env::set_var("PATH", path);
        std::env::set_var("INCODEX_CODESIGN_ENTITLEMENTS", &host);
        std::env::set_var("INCODEX_CODESIGN_CAPTURE", &capture);
        std::env::set_var("INCODEX_CODESIGN_LOG", &log);

        let result = sign_app(&app);
        std::env::remove_var("INCODEX_CODESIGN_ENTITLEMENTS");
        std::env::remove_var("INCODEX_CODESIGN_CAPTURE");
        std::env::remove_var("INCODEX_CODESIGN_LOG");
        assert!(result.is_ok(), "{result:?}");

        let captured = std::fs::read_to_string(capture).unwrap();
        assert!(captured.contains("com.apple.security.device.camera"));
        assert!(captured.contains("com.apple.security.cs.allow-jit"));
        assert!(captured.contains("com.apple.security.cs.disable-library-validation"));
        assert!(!captured.contains("com.apple.developer.team-identifier"));
        assert!(!captured.contains("com.apple.application-identifier"));
        assert!(!captured.contains("keychain-access-groups"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
