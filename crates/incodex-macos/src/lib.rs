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
    for flag in ["-replace", "-insert"] {
        let ok = Command::new("plutil")
            .args([flag, "ElectronAsarIntegrity", "-json", &json])
            .arg(&plist)
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if ok {
            return Ok(());
        }
    }
    Ok(())
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
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .contains("runtime")
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
        .map_err(|err| err.to_string())?;
    if !deep.status.success() {
        return Err(String::from_utf8_lossy(&deep.stderr).trim().to_string());
    }
    for (src, dest) in &stashed {
        ditto(dest, src)?;
    }
    if let Some(root) = stash_root {
        let _ = fs::remove_dir_all(root);
    }
    let outer = Command::new("codesign")
        .args(["--force", "--sign", "-", "--options", "runtime", "--"])
        .arg(app)
        .output()
        .map_err(|err| err.to_string())?;
    if !outer.status.success() {
        return Err(String::from_utf8_lossy(&outer.stderr).trim().to_string());
    }
    if !verify_app(app) {
        return Err("codesign --verify failed after adhoc resign".into());
    }
    Ok(())
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

pub fn notify_launch_services(app: &Path) -> Result<(), String> {
    let _ = Command::new("lsregister")
        .args(["-f", "-R", "-trusted"])
        .arg(app)
        .status();
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {}
}
