use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::windows_system::system_binary_path;

const PACKAGE_NAME: &str = "OpenAI.Codex";
const PACKAGE_FAMILY_SUFFIX: &str = "__2p2nqsd0c76g0";
const PACKAGE_FAMILY_NAME: &str = "OpenAI.Codex_2p2nqsd0c76g0";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsPackageEvidence {
    pub name: String,
    pub package_full_name: String,
    pub package_family_name: String,
    pub applications: Vec<WindowsManifestApplication>,
    pub install_location: PathBuf,
    pub architecture: String,
    pub signature_kind: String,
    pub status: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsManifestApplication {
    pub application_id: String,
    pub application_executable: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsCodexApp {
    pub package_full_name: String,
    pub app_user_model_id: String,
    pub install_location: PathBuf,
    pub executable: PathBuf,
    pub architecture: String,
}

pub fn discover_codex_package() -> Result<WindowsCodexApp, String> {
    let script = r#"[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$package = Get-AppxPackage -Name OpenAI.Codex | Sort-Object Version -Descending | Select-Object -First 1
if ($null -eq $package) { exit 3 }
$manifest = [xml](Get-Content -LiteralPath (Join-Path $package.InstallLocation 'AppxManifest.xml') -Raw)
$applications = @($manifest.Package.Applications.Application | ForEach-Object {
  [PSCustomObject]@{
    applicationId = $_.Id
    applicationExecutable = $_.Executable
  }
})
[PSCustomObject]@{
  name = $package.Name
  packageFullName = $package.PackageFullName
  packageFamilyName = $package.PackageFamilyName
  applications = $applications
  installLocation = $package.InstallLocation
  architecture = $package.Architecture.ToString()
  signatureKind = $package.SignatureKind.ToString()
  status = $package.Status.ToString()
} | ConvertTo-Json -Compress"#;
    let mut command = package_query_command(script)?;
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

pub fn codex_package_full_name_is_installed(package_full_name: &str) -> Result<bool, String> {
    if !package_full_name.starts_with("OpenAI.Codex_")
        || !package_full_name.ends_with(PACKAGE_FAMILY_SUFFIX)
        || package_full_name.contains('\0')
    {
        return Err("Windows package identity is not the official Codex package".to_string());
    }
    let script = r#"$package = Get-AppxPackage -Name OpenAI.Codex | Where-Object { $_.PackageFullName -ceq $env:INCODEX_PACKAGE_FULL_NAME } | Select-Object -First 1
if ($null -eq $package) { exit 3 }
exit 0"#;
    let mut command = package_query_command(script)?;
    command.env("INCODEX_PACKAGE_FULL_NAME", package_full_name);
    let output = command
        .output()
        .map_err(|error| format!("cannot query the installed Windows Codex generation: {error}"))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(3) => Ok(false),
        _ => Err("Windows package generation query failed".to_string()),
    }
}

fn package_query_command(script: &str) -> Result<Command, String> {
    let mut command = Command::new(system_powershell_path()?);
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
    Ok(command)
}

fn system_powershell_path() -> Result<PathBuf, String> {
    system_binary_path(
        Path::new("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe"),
    )
}

pub fn parse_package_evidence(raw: &str) -> Result<WindowsPackageEvidence, String> {
    serde_json::from_str(raw).map_err(|error| format!("invalid Windows package evidence: {error}"))
}

pub fn inspect_codex_package(evidence: WindowsPackageEvidence) -> Result<WindowsCodexApp, String> {
    if evidence.name != PACKAGE_NAME
        || !evidence.package_full_name.starts_with("OpenAI.Codex_")
        || !evidence.package_full_name.ends_with(PACKAGE_FAMILY_SUFFIX)
        || evidence.package_family_name != PACKAGE_FAMILY_NAME
    {
        return Err("Windows package identity is not the official Codex package".to_string());
    }
    if evidence.signature_kind != "Store" || evidence.status != "Ok" {
        return Err("official Codex Microsoft Store package is not healthy".to_string());
    }
    if evidence.architecture.is_empty() {
        return Err("Windows Codex package architecture is missing".to_string());
    }

    let mut applications = evidence.applications.into_iter().filter(|application| {
        application
            .application_executable
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("ChatGPT.exe"))
    });
    let application = applications.next().ok_or_else(|| {
        "Windows package manifest must contain exactly one Codex application executable".to_string()
    })?;
    if applications.next().is_some() {
        return Err(
            "Windows package manifest must contain exactly one Codex application executable"
                .to_string(),
        );
    }
    if !valid_application_id(&application.application_id) {
        return Err("Windows package identity is not the official Codex package".to_string());
    }
    if application.application_executable.as_os_str().is_empty()
        || application.application_executable.is_absolute()
        || !application
            .application_executable
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err("Windows package application executable path is unsafe".to_string());
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
    let executable = install_location.join(&application.application_executable);
    require_file(&executable, "Codex application executable")?;

    Ok(WindowsCodexApp {
        package_full_name: evidence.package_full_name,
        app_user_model_id: format!(
            "{}!{}",
            evidence.package_family_name, application.application_id
        ),
        install_location,
        executable,
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    use super::{package_query_command, PACKAGE_QUERY_CREATION_FLAGS};

    #[test]
    fn package_query_never_creates_a_console_window() {
        assert_eq!(
            PACKAGE_QUERY_CREATION_FLAGS & CREATE_NO_WINDOW,
            CREATE_NO_WINDOW
        );
    }

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
