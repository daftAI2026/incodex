use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

const PACKAGE_NAME: &str = "OpenAI.Codex";
const PACKAGE_FAMILY_SUFFIX: &str = "__2p2nqsd0c76g0";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsPackageEvidence {
    pub name: String,
    pub package_full_name: String,
    pub install_location: PathBuf,
    pub architecture: String,
    pub signature_kind: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsCodexApp {
    pub package_full_name: String,
    pub install_location: PathBuf,
    pub executable: PathBuf,
    pub asar: PathBuf,
    pub asar_unpacked: PathBuf,
    pub architecture: String,
}

pub fn discover_codex_package() -> Result<WindowsCodexApp, String> {
    let script = r#"[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$package = Get-AppxPackage -Name OpenAI.Codex | Sort-Object Version -Descending | Select-Object -First 1
if ($null -eq $package) { exit 3 }
[PSCustomObject]@{
  name = $package.Name
  packageFullName = $package.PackageFullName
  installLocation = $package.InstallLocation
  architecture = $package.Architecture.ToString()
  signatureKind = $package.SignatureKind.ToString()
  status = $package.Status.ToString()
} | ConvertTo-Json -Compress"#;
    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .output()
        .map_err(|error| format!("cannot query the Windows Codex package: {error}"))?;
    if !output.status.success() {
        return Err(if output.status.code() == Some(3) {
            "official Codex Microsoft Store package is not installed".to_string()
        } else {
            "Windows package query failed".to_string()
        });
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| "Windows package query did not return UTF-8".to_string())?;
    inspect_codex_package(parse_package_evidence(
        stdout.trim_start_matches('\u{feff}').trim(),
    )?)
}

pub fn parse_package_evidence(raw: &str) -> Result<WindowsPackageEvidence, String> {
    serde_json::from_str(raw).map_err(|error| format!("invalid Windows package evidence: {error}"))
}

pub fn inspect_codex_package(evidence: WindowsPackageEvidence) -> Result<WindowsCodexApp, String> {
    if evidence.name != PACKAGE_NAME
        || !evidence.package_full_name.starts_with("OpenAI.Codex_")
        || !evidence.package_full_name.ends_with(PACKAGE_FAMILY_SUFFIX)
    {
        return Err("Windows package identity is not the official Codex package".to_string());
    }
    if evidence.signature_kind != "Store" || evidence.status != "Ok" {
        return Err("official Codex Microsoft Store package is not healthy".to_string());
    }
    if evidence.architecture.is_empty() {
        return Err("Windows Codex package architecture is missing".to_string());
    }
    if !evidence.install_location.is_absolute() {
        return Err(format!(
            "Windows Codex package path is not absolute: {}",
            evidence.install_location.display()
        ));
    }

    let install_location = fs::canonicalize(&evidence.install_location).map_err(|error| {
        format!(
            "cannot resolve Windows Codex package {}: {error}",
            evidence.install_location.display()
        )
    })?;
    require_file(&install_location.join("AppxManifest.xml"), "AppX manifest")?;
    let executable = install_location.join("app/ChatGPT.exe");
    let asar = install_location.join("app/resources/app.asar");
    let asar_unpacked = install_location.join("app/resources/app.asar.unpacked");
    require_file(&executable, "Codex executable")?;
    require_file(&asar, "Codex app.asar")?;
    require_dir(&asar_unpacked, "Codex app.asar.unpacked")?;

    Ok(WindowsCodexApp {
        package_full_name: evidence.package_full_name,
        install_location,
        executable,
        asar,
        asar_unpacked,
        architecture: evidence.architecture,
    })
}

fn require_file(path: &Path, label: &str) -> Result<(), String> {
    if path.is_file() {
        Ok(())
    } else {
        Err(format!("{label} not found: {}", path.display()))
    }
}

fn require_dir(path: &Path, label: &str) -> Result<(), String> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(format!("{label} not found: {}", path.display()))
    }
}
