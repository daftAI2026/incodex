use std::cmp::Ordering as VersionOrdering;
use std::fs::{self, OpenOptions};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use crate::parse::ParsedCli;
use crate::windows_system::{system_binary_path, windows_path_for_display};
use crate::windows_update_flow::{
    run_windows_installer_fallback, run_windows_update_pipeline, WindowsUpdateProgress,
};
use incodex_core::windows_path::reject_reparse_ancestors;
use serde::{Deserialize, Serialize};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

const WINDOWS_LATEST_RELEASE_URL: &str =
    "https://api.github.com/repos/daftAI2026/incodex/releases/latest";
const WINDOWS_MAIN_COMMIT_URL: &str =
    "https://api.github.com/repos/daftAI2026/incodex/commits/main";
pub(crate) const WINDOWS_MAIN_INSTALLER_URL: &str =
    "https://raw.githubusercontent.com/daftAI2026/incodex/main/install.ps1";
const MANAGED_BY_STANDALONE_ENV: &str = "INCODEX_MANAGED_BY_STANDALONE";
const MANAGED_PACKAGE_ROOT_ENV: &str = "INCODEX_MANAGED_PACKAGE_ROOT";
const CURRENT_GENERATION_LIMIT: u64 = 64;
const DOWNLOAD_ATTEMPTS: usize = 3;
const DOWNLOAD_RETRY_DELAY: Duration = Duration::from_millis(200);
const RELEASE_METADATA_LIMIT: u64 = 256 * 1024;
const INSTALLER_SCRIPT_LIMIT: u64 = 1024 * 1024;
const RUNTIME_PENDING_NAME: &str = "windows_runtime_update_pending.json";
const RUNTIME_PENDING_LIMIT: u64 = 1024;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsMainSnapshot {
    commit: String,
    installer_url: String,
}

impl WindowsMainSnapshot {
    pub fn commit(&self) -> &str {
        &self.commit
    }

    pub fn installer_url(&self) -> &str {
        &self.installer_url
    }
}

#[derive(Debug, Deserialize)]
struct MainCommit {
    sha: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingWindowsRuntime {
    schema_version: u8,
    cli_version: String,
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
    let latest = crate::stable_release::parse_latest_stable_release(metadata)?;
    let version = format!(
        "{}.{}.{}",
        latest.version[0], latest.version[1], latest.version[2]
    );
    Ok(WindowsStableRelease {
        installer_url: format!(
            "https://raw.githubusercontent.com/daftAI2026/incodex/{}/install.ps1",
            latest.tag
        ),
        download_base: format!(
            "https://github.com/daftAI2026/incodex/releases/download/{}",
            latest.tag
        ),
        tag: latest.tag,
        version,
    })
}

pub fn parse_windows_main_commit(metadata: &[u8]) -> Result<WindowsMainSnapshot, String> {
    let main: MainCommit = serde_json::from_slice(metadata)
        .map_err(|_| "update failed: invalid main commit metadata".to_string())?;
    if main.sha.len() != 40 || !main.sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("update failed: invalid main commit identity".to_string());
    }
    Ok(WindowsMainSnapshot {
        installer_url: format!(
            "https://raw.githubusercontent.com/daftAI2026/incodex/{}/install.ps1",
            main.sha
        ),
        commit: main.sha,
    })
}

pub fn run_runtime(parsed: &ParsedCli) -> Result<(), String> {
    if parsed.dry_run {
        println!("would publish the embedded Runtime without modifying Codex");
        println!("no changes made.");
        return Ok(());
    }

    let _transaction = crate::windows_install_state::acquire_windows_install_state()?;
    let user_root = crate::windows_profile::windows_user_profile()?.join(".incodex");
    if let Some(expected) = read_windows_runtime_pending(&user_root)? {
        if expected != env!("CARGO_PKG_VERSION") {
            return Err(format!(
                "Runtime synchronization expects Incodex {expected}; run inc update from the active installation"
            ));
        }
    }
    let published = crate::windows_runtime::publish_windows_runtime(&user_root)?;
    let runtime_release = published
        .release_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "published Windows Runtime release name is invalid".to_string())?;
    crate::windows_install_state::synchronize_windows_install_runtime_release(
        &user_root,
        runtime_release,
    )?;
    clear_windows_runtime_pending_if_matches(&user_root, env!("CARGO_PKG_VERSION"))?;
    println!("Runtime updated. Codex was not modified.");
    println!(
        "  Runtime  {}",
        windows_path_for_display(&published.release_dir)
    );
    println!("Fully quit and reopen Codex to load the new Runtime.");
    Ok(())
}

fn clear_windows_runtime_pending_if_matches(
    user_root: &Path,
    expected: &str,
) -> Result<(), String> {
    if read_windows_runtime_pending(user_root)?.as_deref() == Some(expected) {
        clear_windows_runtime_pending(user_root)?;
    }
    Ok(())
}

pub fn run_update(parsed: &ParsedCli) -> Result<(), String> {
    if std::env::var(MANAGED_BY_STANDALONE_ENV).as_deref() != Ok("1") {
        return Err(format!(
            "this copy is not a managed Windows installation\n  powershell -ExecutionPolicy Bypass -c \"irm {WINDOWS_MAIN_INSTALLER_URL} | iex\""
        ));
    }
    let package_root = managed_package_root()?;
    println!("update channel: windows standalone");
    if parsed.dry_run {
        println!("would install the latest verified Windows release");
        println!("would publish Runtime with the installed CLI");
        println!("no changes made.");
        return Ok(());
    }

    let running_exe = std::env::current_exe()
        .map_err(|error| format!("cannot locate the running Windows CLI: {error}"))?;
    let user_root = validate_managed_install_identity(&package_root, &running_exe)?;
    let profile = crate::windows_profile::windows_user_profile()?;
    validate_windows_user_root(&user_root, &profile)?;
    let _update = acquire_windows_update_lock(&package_root)?;
    if read_windows_runtime_pending(&user_root)?.is_some() {
        let _lock = acquire_windows_install_lock(&package_root)?;
        repair_pending_runtime(&user_root, &package_root)?;
    }
    let mut progress = crate::spinner::Progress::new();
    let mut stdout = std::io::stdout();
    run_windows_update_pipeline(
        &mut progress,
        &mut stdout,
        |progress| {
            install_latest_stable_release(&package_root, env!("CARGO_PKG_VERSION"), progress)
        },
        || {
            let _lock = acquire_windows_install_lock(&package_root)?;
            let (installed, expected_version) = current_release_executable(&package_root)?;
            verify_cli_version(&installed, &expected_version)?;
            write_windows_runtime_pending(&user_root, &expected_version)?;
            publish_runtime_with(&installed)?;
            if read_windows_runtime_pending(&user_root)?.is_some() {
                return Err(
                    "CLI was updated, but Runtime synchronization remains pending".to_string(),
                );
            }
            Ok(())
        },
    )
}

#[derive(Debug)]
pub struct WindowsInstallLock {
    _files: Vec<fs::File>,
}

pub fn acquire_windows_install_lock(package_root: &Path) -> Result<WindowsInstallLock, String> {
    let lock = windows_install_lock_path(package_root)?;
    let stable = acquire_windows_channel_lock_file(
        lock.parent()
            .ok_or_else(|| "Windows installation lock has no parent".to_string())?,
        lock.file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "Windows installation lock name is invalid".to_string())?,
        "Windows installation lock",
        "another Incodex install or update is already running",
    )?;
    let legacy = acquire_windows_channel_lock_file(
        package_root,
        "install.lock",
        "legacy Windows installation lock",
        "another Incodex install or update is already running",
    )?;
    Ok(WindowsInstallLock {
        _files: vec![stable, legacy],
    })
}

pub fn windows_install_lock_path(package_root: &Path) -> Result<PathBuf, String> {
    let packages = package_root
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "packages"))
        .ok_or_else(|| "managed Windows package root has an invalid layout".to_string())?;
    if package_root
        .file_name()
        .is_none_or(|name| name != "standalone")
    {
        return Err("managed Windows package root has an invalid layout".to_string());
    }
    let user_root = packages
        .parent()
        .ok_or_else(|| "managed Windows package root has no user root".to_string())?;
    ensure_regular_non_reparse(user_root, "managed Windows user root")?;
    incodex_core::windows_session::verify_private_acl(user_root)?;
    Ok(user_root.join("standalone-install.lock"))
}

pub fn acquire_windows_update_lock(package_root: &Path) -> Result<WindowsInstallLock, String> {
    let file = acquire_windows_channel_lock_file(
        package_root,
        "update.lock",
        "Windows update lock",
        "another Incodex update is already running",
    )?;
    Ok(WindowsInstallLock { _files: vec![file] })
}

fn acquire_windows_channel_lock_file(
    package_root: &Path,
    name: &str,
    description: &str,
    busy: &str,
) -> Result<fs::File, String> {
    ensure_regular_non_reparse(package_root, "managed Windows package root")?;
    incodex_core::windows_session::verify_private_acl(package_root)?;
    let lock_path = package_root.join(name);
    match fs::symlink_metadata(&lock_path) {
        Ok(_) => {
            ensure_regular_non_reparse(&lock_path, description)?;
            incodex_core::windows_session::verify_private_acl(&lock_path)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("cannot inspect {description}: {error}")),
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(&lock_path)
        .map_err(|_| busy.to_string())?;
    incodex_core::windows_session::apply_private_windows_acl(&lock_path)?;
    incodex_core::windows_session::verify_private_acl(&lock_path)?;
    Ok(file)
}

pub fn write_windows_runtime_pending(user_root: &Path, version: &str) -> Result<(), String> {
    validate_stable_version(version)?;
    let user_root = incodex_core::windows_session::ensure_private_windows_dir(user_root)?;
    let cache =
        incodex_core::windows_session::ensure_private_windows_dir(&user_root.join("cache"))?;
    let body = serde_json::to_vec_pretty(&PendingWindowsRuntime {
        schema_version: 1,
        cli_version: version.to_string(),
    })
    .map_err(|error| format!("cannot serialize Runtime update state: {error}"))?;
    crate::windows_runtime::replace_private_file(&cache, &cache.join(RUNTIME_PENDING_NAME), &body)
}

pub fn read_windows_runtime_pending(user_root: &Path) -> Result<Option<String>, String> {
    let Some(path) = windows_runtime_pending_path(user_root)? else {
        return Ok(None);
    };
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect Runtime update state: {error}")),
    };
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() > RUNTIME_PENDING_LIMIT
    {
        return Err("Runtime update state is invalid".to_string());
    }
    incodex_core::windows_session::verify_private_acl(&path)?;
    let body =
        fs::read(&path).map_err(|error| format!("cannot read Runtime update state: {error}"))?;
    let pending: PendingWindowsRuntime =
        serde_json::from_slice(&body).map_err(|_| "Runtime update state is invalid".to_string())?;
    if pending.schema_version != 1 {
        return Err("Runtime update state schema is unsupported".to_string());
    }
    validate_stable_version(&pending.cli_version)
        .map_err(|_| "Runtime update state has an invalid CLI version".to_string())?;
    Ok(Some(pending.cli_version))
}

pub fn clear_windows_runtime_pending(user_root: &Path) -> Result<(), String> {
    let Some(path) = windows_runtime_pending_path(user_root)? else {
        return Ok(());
    };
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot inspect Runtime update state: {error}")),
        Ok(metadata)
            if metadata.is_file()
                && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 =>
        {
            incodex_core::windows_session::verify_private_acl(&path)?;
            fs::remove_file(&path)
                .map_err(|error| format!("cannot clear Runtime update state: {error}"))
        }
        Ok(_) => Err("Runtime update state is not a regular file".to_string()),
    }
}

fn windows_runtime_pending_path(user_root: &Path) -> Result<Option<PathBuf>, String> {
    let cache = user_root.join("cache");
    let metadata = match fs::symlink_metadata(&cache) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot inspect Runtime update directory: {error}")),
    };
    reject_reparse_ancestors(&cache)?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("Runtime update directory is not a regular directory".to_string());
    }
    incodex_core::windows_session::verify_private_acl(&cache)?;
    Ok(Some(cache.join(RUNTIME_PENDING_NAME)))
}

fn repair_pending_runtime(user_root: &Path, package_root: &Path) -> Result<(), String> {
    let Some(expected) = read_windows_runtime_pending(user_root)? else {
        return Ok(());
    };
    let (installed, current) = current_release_executable(package_root)?;
    if current != expected {
        return Err(format!(
            "Runtime synchronization expects Incodex {expected}, but the active generation is {current}"
        ));
    }
    verify_cli_version(&installed, &expected)?;
    publish_runtime_with(&installed)?;
    if read_windows_runtime_pending(user_root)?.is_some() {
        return Err("Runtime synchronization remains pending".to_string());
    }
    Ok(())
}

pub fn validate_managed_install_identity(
    package_root: &Path,
    running_exe: &Path,
) -> Result<PathBuf, String> {
    let packages = package_root
        .parent()
        .filter(|path| path.file_name().is_some_and(|name| name == "packages"))
        .ok_or_else(|| "managed Windows package root has an invalid layout".to_string())?;
    if package_root
        .file_name()
        .is_none_or(|name| name != "standalone")
    {
        return Err("managed Windows package root has an invalid layout".to_string());
    }
    let user_root = packages
        .parent()
        .ok_or_else(|| "managed Windows package root has no user root".to_string())?;
    let (expected_exe, expected_version) = current_release_executable(package_root)?;
    ensure_regular_non_reparse(package_root, "managed Windows package root")?;
    ensure_regular_non_reparse(&expected_exe, "managed Windows CLI")?;
    ensure_regular_non_reparse(running_exe, "running Windows CLI")?;
    let expected = fs::canonicalize(&expected_exe)
        .map_err(|error| format!("cannot resolve the managed Windows CLI: {error}"))?;
    let running = fs::canonicalize(running_exe)
        .map_err(|error| format!("cannot resolve the running Windows CLI: {error}"))?;
    if expected != running {
        return Err("running Windows CLI does not match the managed generation".to_string());
    }
    verify_cli_version(&running, &expected_version)?;
    Ok(user_root.to_path_buf())
}

pub fn validate_windows_user_root(user_root: &Path, profile: &Path) -> Result<(), String> {
    if !user_root.is_absolute() || !profile.is_absolute() {
        return Err("managed Windows installation path is not absolute".to_string());
    }
    let expected = profile.join(".incodex");
    let same_text = user_root
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected.to_string_lossy());
    let same_canonical = fs::canonicalize(user_root)
        .and_then(|actual| fs::canonicalize(&expected).map(|expected| actual == expected))
        .unwrap_or(false);
    if same_text || same_canonical {
        Ok(())
    } else {
        Err("managed Windows installation is outside the current token profile".to_string())
    }
}

pub(crate) fn managed_package_root() -> Result<PathBuf, String> {
    let value = std::env::var_os(MANAGED_PACKAGE_ROOT_ENV).ok_or_else(|| {
        "managed Windows installation did not provide its package root".to_string()
    })?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err("managed Windows package root is not absolute".to_string());
    }
    Ok(path)
}

pub fn windows_release_ordering(
    current: &str,
    latest: &WindowsStableRelease,
) -> Result<VersionOrdering, String> {
    let current = crate::stable_release::parse_stable_version(current)
        .ok_or_else(|| format!("invalid stable Incodex version: {current}"))?;
    let latest = crate::stable_release::parse_stable_version(latest.version())
        .ok_or_else(|| format!("invalid stable Incodex version: {}", latest.version()))?;
    Ok(latest.cmp(&current))
}

fn install_latest_stable_release(
    package_root: &Path,
    current: &str,
    progress: &mut impl WindowsUpdateProgress,
) -> Result<(VersionOrdering, String), String> {
    let work = UpdateWorkDirectory::create(package_root)?;
    let metadata_path = work.path.join("latest.json");
    download_with_powershell(
        WINDOWS_LATEST_RELEASE_URL,
        &metadata_path,
        "release metadata",
        RELEASE_METADATA_LIMIT,
    )?;
    let metadata = fs::read(&metadata_path)
        .map_err(|error| format!("update failed: cannot read release metadata: {error}"))?;
    let release = parse_windows_stable_release(&metadata)?;
    let ordering = windows_release_ordering(current, &release)?;
    if ordering != VersionOrdering::Greater {
        return Ok((ordering, release.tag().to_string()));
    }

    let tagged_installer = work.path.join("install.stable.ps1");
    download_with_powershell(
        release.installer_url(),
        &tagged_installer,
        "stable installer",
        INSTALLER_SCRIPT_LIMIT,
    )?;
    let mut stderr = std::io::stderr();
    run_windows_installer_fallback(
        progress,
        &mut stderr,
        || run_windows_installer(&tagged_installer, &release),
        || {
            let main_metadata_path = work.path.join("main.json");
            download_with_powershell(
                WINDOWS_MAIN_COMMIT_URL,
                &main_metadata_path,
                "main commit metadata",
                RELEASE_METADATA_LIMIT,
            )?;
            let main_metadata = fs::read(&main_metadata_path).map_err(|error| {
                format!("update failed: cannot read main commit metadata: {error}")
            })?;
            let snapshot = parse_windows_main_commit(&main_metadata)?;
            let compatibility_installer = work.path.join("install.compatibility.ps1");
            download_with_powershell(
                snapshot.installer_url(),
                &compatibility_installer,
                "compatibility installer",
                INSTALLER_SCRIPT_LIMIT,
            )?;
            run_windows_installer(&compatibility_installer, &release)
        },
    )?;
    Ok((VersionOrdering::Greater, release.tag().to_string()))
}

fn run_windows_installer(installer: &Path, release: &WindowsStableRelease) -> Result<(), String> {
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
        .env("INCODEX_NON_INTERACTIVE", "1")
        .env("INCODEX_INTERNAL_UPDATE", "1")
        .env("INCODEX_DOWNLOAD_BASE", release.download_base())
        .env("INCODEX_EXPECTED_VERSION", release.version());
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("could not start the Windows installer: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_failure("Windows installer", &output))
    }
}

fn ensure_regular_non_reparse(path: &Path, label: &str) -> Result<(), String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("cannot inspect {label}: {error}"))?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (!metadata.is_dir() && !metadata.is_file())
    {
        return Err(format!("{label} is not a regular filesystem object"));
    }
    Ok(())
}

fn download_with_powershell(
    url: &str,
    destination: &Path,
    label: &str,
    limit: u64,
) -> Result<(), String> {
    let powershell = system_binary_path("WindowsPowerShell/v1.0/powershell.exe")?;
    let destination = windows_powershell_path(destination)?;
    let mut last_error = String::new();
    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        let output = Command::new(&powershell)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                "$ProgressPreference='SilentlyContinue'; Invoke-WebRequest -UseBasicParsing -TimeoutSec 30 -Uri $env:INCODEX_UPDATE_URI -OutFile $env:INCODEX_UPDATE_OUT",
            ])
            .env("INCODEX_UPDATE_URI", url)
            .env("INCODEX_UPDATE_OUT", &destination)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("update failed: could not start PowerShell: {error}"))?;
        if output.status.success() {
            let length = fs::metadata(&destination)
                .map_err(|error| format!("update failed: cannot inspect {label}: {error}"))?
                .len();
            validate_windows_download_size(label, length, limit)?;
            return Ok(());
        }
        let detail = String::from_utf8_lossy(&output.stderr);
        last_error = if detail.trim().is_empty() {
            output.status.to_string()
        } else {
            detail.trim().to_string()
        };
        let _ = fs::remove_file(&destination);
        if attempt < DOWNLOAD_ATTEMPTS {
            thread::sleep(DOWNLOAD_RETRY_DELAY);
        }
    }
    Err(format!(
        "update failed: could not download {label}: {last_error}"
    ))
}

pub fn windows_powershell_path(path: &Path) -> Result<String, String> {
    if !path.is_absolute() {
        return Err("update failed: PowerShell download path is not absolute".to_string());
    }
    let path = windows_path_for_display(path);
    if path.starts_with(r"\\?\") {
        return Err("update failed: PowerShell cannot write this Windows path form".to_string());
    }
    Ok(path)
}

pub fn validate_windows_download_size(label: &str, length: u64, limit: u64) -> Result<(), String> {
    if length == 0 || length > limit {
        Err(format!("update failed: {label} is empty or too large"))
    } else {
        Ok(())
    }
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

pub(crate) fn native_open_executable_for_runtime(
    user_root: &Path,
    helper_executable: &Path,
    runtime_release: &str,
) -> Result<PathBuf, String> {
    select_native_open_executable(
        helper_executable,
        env!("CARGO_PKG_VERSION"),
        runtime_release,
        || {
            let package_root = user_root.join("packages").join("standalone");
            let (executable, version) = current_release_executable(&package_root)?;
            verify_cli_version(&executable, &version)?;
            Ok((executable, version))
        },
    )
}

fn select_native_open_executable<F>(
    helper_executable: &Path,
    helper_version: &str,
    runtime_release: &str,
    current_managed_release: F,
) -> Result<PathBuf, String>
where
    F: FnOnce() -> Result<(PathBuf, String), String>,
{
    let runtime_version = runtime_release
        .split_once('-')
        .map_or(runtime_release, |(version, _)| version);
    validate_stable_version(runtime_version)?;
    validate_stable_version(helper_version)?;
    if runtime_version == helper_version {
        return Ok(helper_executable.to_path_buf());
    }

    let (executable, managed_version) = current_managed_release()?;
    if managed_version != runtime_version {
        return Err(format!(
            "installed Windows Runtime {runtime_version} has no matching managed CLI generation"
        ));
    }
    Ok(executable)
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
    let output = Command::new(installed)
        .arg("runtime")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("could not publish Runtime with the installed CLI: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "CLI was updated, but Runtime synchronization failed\n{}",
            command_failure("Runtime command", &output)
        ))
    }
}

fn command_failure(label: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = [stderr.trim(), stdout.trim()]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if detail.is_empty() {
        format!("{label} failed with {}", output.status)
    } else {
        format!("{label} failed with {}\n{detail}", output.status)
    }
}

#[cfg(test)]
mod tests {
    use super::select_native_open_executable;
    use std::path::{Path, PathBuf};

    #[test]
    fn installed_bridge_uses_the_cli_generation_that_published_runtime() {
        let helper = Path::new(r"C:\Users\Kid\.incodex\windows\i\old\i.exe");
        let active =
            PathBuf::from(r"C:\Users\Kid\.incodex\packages\standalone\releases\1.0.0\incodex.exe");
        let selected =
            select_native_open_executable(helper, "0.9.0", "1.0.0-0123456789abcdef", || {
                Ok((active.clone(), "1.0.0".to_string()))
            })
            .expect("runtime generation selects its managed CLI");

        assert_eq!(selected, active);
    }

    #[test]
    fn installed_bridge_keeps_its_helper_for_the_same_runtime_version() {
        let helper = Path::new(r"C:\Users\Kid\.incodex\windows\i\current\i.exe");
        let selected =
            select_native_open_executable(helper, "1.0.0", "1.0.0-fedcba9876543210", || {
                panic!("same-version runtime must not require a standalone install")
            })
            .expect("same-version helper remains self-contained");

        assert_eq!(selected, helper);
    }
}

fn validate_stable_version(version: &str) -> Result<(), String> {
    if crate::stable_release::parse_stable_version(version).is_none() {
        return Err(format!("invalid stable Incodex version: {version}"));
    }
    Ok(())
}
