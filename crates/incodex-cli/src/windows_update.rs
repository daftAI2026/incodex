use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::parse::ParsedCli;
use crate::windows_system::{system_binary_path, windows_path_for_display};

const WINDOWS_INSTALLER_URL: &str =
    "https://raw.githubusercontent.com/daftAI2026/incodex/main/install.ps1";
const MANAGED_BY_STANDALONE_ENV: &str = "INCODEX_MANAGED_BY_STANDALONE";
const MANAGED_PACKAGE_ROOT_ENV: &str = "INCODEX_MANAGED_PACKAGE_ROOT";
const INSTALLER_PATH_ENV: &str = "INCODEX_WINDOWS_INSTALLER_PATH";
const CURRENT_GENERATION_LIMIT: u64 = 64;

pub const WINDOWS_X64_RELEASE_ASSET: &str = "incodex-windows-x64.exe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsStandaloneLayout {
    user_root: PathBuf,
}

impl WindowsStandaloneLayout {
    pub fn new(user_root: &Path) -> Self {
        Self {
            user_root: user_root.to_path_buf(),
        }
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.user_root.join("bin")
    }

    pub fn package_root(&self) -> PathBuf {
        self.user_root.join("packages").join("standalone")
    }

    pub fn release_executable(&self, version: &str) -> Result<PathBuf, String> {
        validate_stable_version(version)?;
        Ok(self
            .package_root()
            .join("releases")
            .join(version)
            .join("incodex.exe"))
    }

    pub fn primary_launcher(&self) -> PathBuf {
        self.bin_dir().join("incodex.cmd")
    }

    pub fn alias_launcher(&self) -> PathBuf {
        self.bin_dir().join("inc.cmd")
    }
}

pub fn windows_release_asset(architecture: &str) -> Result<&'static str, String> {
    match architecture.to_ascii_lowercase().as_str() {
        "x86_64" | "amd64" | "x64" => Ok(WINDOWS_X64_RELEASE_ASSET),
        other => Err(format!("unsupported Windows architecture: {other}")),
    }
}

pub fn expected_release_sha256(manifest: &str, asset: &str) -> Result<String, String> {
    let matches = manifest
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let digest = fields.next()?;
            let name = fields.next()?;
            (name == asset && fields.next().is_none()).then_some(digest)
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(format!(
            "SHA256SUMS must contain exactly one entry for {asset}"
        ));
    }
    let digest = matches[0];
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("SHA256SUMS contains an invalid digest for {asset}"));
    }
    Ok(digest.to_ascii_lowercase())
}

pub fn run_runtime(parsed: &ParsedCli) -> Result<(), String> {
    if parsed.dry_run {
        println!("would publish the embedded Runtime without modifying Codex");
        println!("no changes made.");
        return Ok(());
    }

    let user_root = crate::windows_profile::windows_user_profile()?.join(".incodex");
    let published = crate::windows_runtime::publish_windows_runtime(&user_root)?;
    println!("Runtime updated. Codex was not modified.");
    println!(
        "  Runtime  {}",
        windows_path_for_display(&published.release_dir)
    );
    println!("Fully quit and reopen Codex to load the new Runtime.");
    Ok(())
}

pub fn run_update(parsed: &ParsedCli) -> Result<(), String> {
    if std::env::var(MANAGED_BY_STANDALONE_ENV).as_deref() != Ok("1") {
        return Err(format!(
            "this copy is not a managed Windows installation\n  powershell -ExecutionPolicy Bypass -c \"irm {WINDOWS_INSTALLER_URL} | iex\""
        ));
    }
    let package_root = managed_package_root()?;
    let display_command = format!(
        "powershell -ExecutionPolicy Bypass -c \"$env:INCODEX_NON_INTERACTIVE='1'; irm {WINDOWS_INSTALLER_URL} | iex\""
    );
    println!("Updating Incodex via `{display_command}`...\n");
    if parsed.dry_run {
        println!("would install the latest verified Windows release");
        println!("would publish Runtime with the installed CLI");
        println!("no changes made.");
        return Ok(());
    }

    run_windows_installer()?;
    let (installed, expected_version) = current_release_executable(&package_root)?;
    verify_cli_version(&installed, &expected_version)?;
    publish_runtime_with(&installed)?;
    println!("\n🎉 Update ran successfully! Please quit and reopen Codex.");
    Ok(())
}

fn managed_package_root() -> Result<PathBuf, String> {
    let value = std::env::var_os(MANAGED_PACKAGE_ROOT_ENV).ok_or_else(|| {
        "managed Windows installation did not provide its package root".to_string()
    })?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err("managed Windows package root is not absolute".to_string());
    }
    Ok(path)
}

fn run_windows_installer() -> Result<(), String> {
    let powershell = system_binary_path("WindowsPowerShell/v1.0/powershell.exe")?;
    let mut command = Command::new(powershell);
    command.args(["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass"]);
    if let Some(installer) = std::env::var_os(INSTALLER_PATH_ENV) {
        command.arg("-File").arg(installer);
    } else {
        command.args([
            "-Command",
            &format!("$env:INCODEX_NON_INTERACTIVE='1'; irm '{WINDOWS_INSTALLER_URL}' | iex"),
        ]);
    }
    let status = command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| format!("could not start the Windows installer: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Windows installer failed with {status}"))
    }
}

fn current_release_executable(package_root: &Path) -> Result<(PathBuf, String), String> {
    let current = package_root.join("current");
    let metadata = fs::symlink_metadata(&current)
        .map_err(|error| format!("cannot inspect the installed generation: {error}"))?;
    if !metadata.file_type().is_file() || metadata.len() > CURRENT_GENERATION_LIMIT {
        return Err("installed Windows generation marker is invalid".to_string());
    }
    let version = fs::read_to_string(&current)
        .map_err(|error| format!("cannot read the installed generation: {error}"))?;
    let version = version.trim();
    validate_stable_version(version)?;
    let executable = package_root
        .join("releases")
        .join(version)
        .join("incodex.exe");
    let metadata = fs::symlink_metadata(&executable)
        .map_err(|error| format!("cannot inspect the installed Windows CLI: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("installed Windows CLI is not a regular file".to_string());
    }
    Ok((executable, version.to_string()))
}

fn verify_cli_version(installed: &Path, expected: &str) -> Result<(), String> {
    let output = Command::new(installed)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("could not verify the installed Windows CLI: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "installed Windows CLI version probe failed with {}",
            output.status
        ));
    }
    let prefix = "Incodex version ";
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .ok_or_else(|| "installed Windows CLI did not report its version".to_string())?;
    validate_stable_version(version)?;
    if version == expected {
        Ok(())
    } else {
        Err(format!(
            "installed Windows CLI reports {version}, expected {expected}"
        ))
    }
}

fn publish_runtime_with(installed: &Path) -> Result<(), String> {
    let status = Command::new(installed)
        .arg("runtime")
        .stdin(Stdio::null())
        .status()
        .map_err(|error| format!("could not publish Runtime with the installed CLI: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "CLI was updated, but Runtime synchronization failed with {status}"
        ))
    }
}

fn validate_stable_version(version: &str) -> Result<(), String> {
    let parts = version.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(format!("invalid stable Incodex version: {version}"));
    }
    Ok(())
}
