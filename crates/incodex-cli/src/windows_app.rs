use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

const PACKAGE_NAME: &str = "OpenAI.Codex";
const PACKAGE_FAMILY_SUFFIX: &str = "__2p2nqsd0c76g0";
const PACKAGE_FAMILY_NAME: &str = "OpenAI.Codex_2p2nqsd0c76g0";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsPackageEvidence {
    pub name: String,
    pub package_full_name: String,
    pub package_family_name: String,
    pub application_id: String,
    pub install_location: PathBuf,
    pub architecture: String,
    pub signature_kind: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsCodexApp {
    pub package_full_name: String,
    pub app_user_model_id: String,
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
$manifest = [xml](Get-Content -LiteralPath (Join-Path $package.InstallLocation 'AppxManifest.xml') -Raw)
$application = @($manifest.Package.Applications.Application) | Select-Object -First 1
if ($null -eq $application) { exit 4 }
[PSCustomObject]@{
  name = $package.Name
  packageFullName = $package.PackageFullName
  packageFamilyName = $package.PackageFamilyName
  applicationId = $application.Id
  installLocation = $package.InstallLocation
  architecture = $package.Architecture.ToString()
  signatureKind = $package.SignatureKind.ToString()
  status = $package.Status.ToString()
} | ConvertTo-Json -Compress"#;
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .env_remove("HOME")
        .env_remove("USERPROFILE")
        .env_remove("LOCALAPPDATA");
    let output = command
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
        || evidence.package_family_name != PACKAGE_FAMILY_NAME
        || !valid_application_id(&evidence.application_id)
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
        app_user_model_id: format!(
            "{}!{}",
            evidence.package_family_name, evidence.application_id
        ),
        install_location,
        executable,
        asar,
        asar_unpacked,
        architecture: evidence.architecture,
    })
}

fn valid_application_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::package_query_command;

    #[test]
    fn package_query_uses_an_absolute_system_powershell() {
        let command = package_query_command("exit 0").expect("build package query command");
        let program = Path::new(command.get_program());

        assert!(program.is_absolute(), "{}", program.display());
        assert!(
            program.ends_with("WindowsPowerShell/v1.0/powershell.exe"),
            "{}",
            program.display()
        );
    }
}
