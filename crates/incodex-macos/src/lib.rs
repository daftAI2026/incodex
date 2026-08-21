//! codesign, plist, ditto, Launch Services, and quit.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod signing;
mod signing_policy;
mod entitlements;
pub use signing::*;
pub use signing_policy::validate_generic_nested_components;

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
    use std::os::unix::fs::PermissionsExt;
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

        static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
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
}
