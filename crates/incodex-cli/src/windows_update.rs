use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use crate::parse::ParsedCli;
use crate::windows_system::{system_binary_path, windows_path_for_display};
use serde::Deserialize;

const WINDOWS_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/daftAI2026/incodex/releases/latest";
const WINDOWS_MAIN_INSTALLER_URL: &str =
    "https://raw.githubusercontent.com/daftAI2026/incodex/main/install.ps1";
const MANAGED_BY_STANDALONE_ENV: &str = "INCODEX_MANAGED_BY_STANDALONE";
const MANAGED_PACKAGE_ROOT_ENV: &str = "INCODEX_MANAGED_PACKAGE_ROOT";
const INSTALLER_PATH_ENV: &str = "INCODEX_WINDOWS_INSTALLER_PATH";
const CURRENT_GENERATION_LIMIT: u64 = 64;
const DOWNLOAD_ATTEMPTS: usize = 3;
const DOWNLOAD_RETRY_DELAY: Duration = Duration::from_millis(200);
static UPDATE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub const WINDOWS_X64_RELEASE_ASSET: &str = "incodex-windows-x64.exe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsStableRelease {
    tag: String,
    version: String,
    installer_url: String,
    download_base: String,
}

impl WindowsStableRelease {
    pub fn tag(&self) -> &str {
        &self.tag
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn installer_url(&self) -> &str {
        &self.installer_url
    }

    pub fn download_base(&self) -> &str {
        &self.download_base
    }
}

#[derive(Debug, Deserialize)]
struct LatestRelease {
    tag_name: String,
}

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

pub fn parse_windows_stable_release(metadata: &[u8]) -> Result<WindowsStableRelease, String> {
    let latest: LatestRelease = serde_json::from_slice(metadata)
        .map_err(|_| "update failed: invalid latest release metadata".to_string())?;
    let version = latest.tag_name.strip_prefix('v').ok_or_else(|| {
        format!(
            "update failed: invalid latest release tag: {}",
            latest.tag_name
        )
    })?;
    validate_stable_version(version).map_err(|_| {
        format!(
            "update failed: invalid latest release tag: {}",
            latest.tag_name
        )
    })?;
    if latest.tag_name != format!("v{version}") {
        return Err(format!(
            "update failed: invalid latest release tag: {}",
            latest.tag_name
        ));
    }
    let version = version.to_string();
    Ok(WindowsStableRelease {
        installer_url: format!(
            "https://raw.githubusercontent.com/daftAI2026/incodex/{}/install.ps1",
            latest.tag_name
        ),
        download_base: format!(
            "https://github.com/daftAI2026/incodex/releases/download/{}",
            latest.tag_name
        ),
        tag: latest.tag_name,
        version,
    })
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
            "this copy is not a managed Windows installation\n  powershell -ExecutionPolicy Bypass -c \"irm {WINDOWS_MAIN_INSTALLER_URL} | iex\""
        ));
    }
    let package_root = managed_package_root()?;
    let display_command = format!(
        "powershell -ExecutionPolicy Bypass -c \"$env:INCODEX_NON_INTERACTIVE='1'; irm {WINDOWS_MAIN_INSTALLER_URL} | iex\""
    );
    println!("Updating Incodex via `{display_command}`...\n");
    if parsed.dry_run {
        println!("would install the latest verified Windows release");
        println!("would publish Runtime with the installed CLI");
        println!("no changes made.");
        return Ok(());
    }

    if let Some(installer) = std::env::var_os(INSTALLER_PATH_ENV) {
        run_windows_installer(Path::new(&installer), None)?;
    } else {
        install_latest_stable_release(&package_root)?;
    }
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

fn install_latest_stable_release(package_root: &Path) -> Result<(), String> {
    let work = UpdateWorkDirectory::create(package_root)?;
    let metadata_path = work.path.join("latest.json");
    download_with_powershell(
        WINDOWS_LATEST_RELEASE_URL,
        &metadata_path,
        "release metadata",
    )?;
    let metadata = fs::read(&metadata_path)
        .map_err(|error| format!("update failed: cannot read release metadata: {error}"))?;
    let release = parse_windows_stable_release(&metadata)?;

    let tagged_installer = work.path.join("install.stable.ps1");
    download_with_powershell(
        release.installer_url(),
        &tagged_installer,
        "stable installer",
    )?;
    let first = run_windows_installer(&tagged_installer, Some(&release));
    if first.is_ok() {
        return Ok(());
    }

    eprintln!(
        "Stable installer did not complete: {}",
        first.expect_err("failed installer result")
    );
    let compatibility_installer = work.path.join("install.compatibility.ps1");
    download_with_powershell(
        WINDOWS_MAIN_INSTALLER_URL,
        &compatibility_installer,
        "compatibility installer",
    )?;
    run_windows_installer(&compatibility_installer, Some(&release))
}

fn run_windows_installer(
    installer: &Path,
    release: Option<&WindowsStableRelease>,
) -> Result<(), String> {
    let powershell = system_binary_path("WindowsPowerShell/v1.0/powershell.exe")?;
    let mut command = Command::new(powershell);
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(installer)
        .env("INCODEX_NON_INTERACTIVE", "1");
    if let Some(release) = release {
        command
            .env("INCODEX_DOWNLOAD_BASE", release.download_base())
            .env("INCODEX_EXPECTED_VERSION", release.version());
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

fn download_with_powershell(url: &str, destination: &Path, label: &str) -> Result<(), String> {
    let powershell = system_binary_path("WindowsPowerShell/v1.0/powershell.exe")?;
    let mut last_error = String::new();
    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        let output = Command::new(&powershell)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                "$ProgressPreference='SilentlyContinue'; Invoke-WebRequest -UseBasicParsing -Uri $env:INCODEX_UPDATE_URI -OutFile $env:INCODEX_UPDATE_OUT",
            ])
            .env("INCODEX_UPDATE_URI", url)
            .env("INCODEX_UPDATE_OUT", destination)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("update failed: could not start PowerShell: {error}"))?;
        if output.status.success() {
            return Ok(());
        }
        let detail = String::from_utf8_lossy(&output.stderr);
        last_error = if detail.trim().is_empty() {
            output.status.to_string()
        } else {
            detail.trim().to_string()
        };
        let _ = fs::remove_file(destination);
        if attempt < DOWNLOAD_ATTEMPTS {
            thread::sleep(DOWNLOAD_RETRY_DELAY);
        }
    }
    Err(format!(
        "update failed: could not download {label}: {last_error}"
    ))
}

struct UpdateWorkDirectory {
    path: PathBuf,
}

impl UpdateWorkDirectory {
    fn create(package_root: &Path) -> Result<Self, String> {
        let path = package_root.join(format!(
            ".update-{}-{}",
            std::process::id(),
            UPDATE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let path = incodex_core::windows_session::ensure_private_windows_dir(&path)?;
        Ok(Self { path })
    }
}

impl Drop for UpdateWorkDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
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
        || parts.iter().any(|part| {
            part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(format!("invalid stable Incodex version: {version}"));
    }
    Ok(())
}
