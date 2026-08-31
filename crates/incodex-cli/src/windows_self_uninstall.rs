use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::fs::MetadataExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_core::windows_path::reject_reparse_ancestors;
use incodex_core::{format_kv, format_ok, format_step, format_warn};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use crate::parse::ParsedCli;
use crate::windows_install::{
    capture_windows_uninstall_approval, uninstall_windows_runtime_approved_with,
    WindowsUninstallOutcome,
};
use crate::windows_update::{
    acquire_windows_update_lock, managed_package_root, validate_managed_install_identity,
    validate_windows_user_root, WindowsStandaloneLayout, WINDOWS_MAIN_INSTALLER_URL,
};

const MANAGED_BY_STANDALONE_ENV: &str = "INCODEX_MANAGED_BY_STANDALONE";
const CLEANUP_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$Owner = Get-Process -Id ([int]$env:INCODEX_SELF_UNINSTALL_PID) -ErrorAction SilentlyContinue
if ($null -ne $Owner) {
    $Owner.WaitForExit()
}

try {
    if (Test-Path -LiteralPath $env:INCODEX_SELF_UNINSTALL_PACKAGE) {
        Remove-Item -LiteralPath $env:INCODEX_SELF_UNINSTALL_PACKAGE -Recurse -Force -ErrorAction Stop
    }
    if (Test-Path -LiteralPath $env:INCODEX_SELF_UNINSTALL_PRIMARY) {
        Remove-Item -LiteralPath $env:INCODEX_SELF_UNINSTALL_PRIMARY -Force -ErrorAction Stop
    }
    if (Test-Path -LiteralPath $env:INCODEX_SELF_UNINSTALL_ALIAS) {
        Remove-Item -LiteralPath $env:INCODEX_SELF_UNINSTALL_ALIAS -Force -ErrorAction Stop
    }

    if ($env:INCODEX_SELF_UNINSTALL_REMOVE_PATH -eq '1') {
        $Bin = $env:INCODEX_SELF_UNINSTALL_BIN.TrimEnd('\')
        $Current = [Environment]::GetEnvironmentVariable('Path', 'User')
        $Entries = @($Current -split ';' | Where-Object {
            -not [string]::IsNullOrWhiteSpace($_) -and $_.TrimEnd('\') -ine $Bin
        })
        [Environment]::SetEnvironmentVariable('Path', ($Entries -join ';'), 'User')
    }

    Remove-Item -LiteralPath $env:INCODEX_SELF_UNINSTALL_BIN -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $env:INCODEX_SELF_UNINSTALL_PACKAGES -Force -ErrorAction SilentlyContinue
} catch {
    [IO.File]::WriteAllText(
        $env:INCODEX_SELF_UNINSTALL_ERROR,
        $_.Exception.ToString(),
        (New-Object Text.UTF8Encoding($false))
    )
} finally {
    Remove-Item -LiteralPath $env:INCODEX_SELF_UNINSTALL_SCRIPT -Force -ErrorAction SilentlyContinue
}
"#;

static SCRIPT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn run_self_uninstall(parsed: &ParsedCli) -> Result<(), String> {
    if std::env::var(MANAGED_BY_STANDALONE_ENV).as_deref() != Ok("1") {
        return Err(format!(
            "this copy is not a managed Windows installation\n  powershell -ExecutionPolicy Bypass -c \"irm {WINDOWS_MAIN_INSTALLER_URL} | iex\""
        ));
    }

    let package_root = managed_package_root()?;
    let running_exe = std::env::current_exe()
        .map_err(|error| format!("cannot locate the running Windows CLI: {error}"))?;
    let user_root = validate_managed_install_identity(&package_root, &running_exe)?;
    let profile = crate::windows_profile::windows_user_profile()?;
    validate_windows_user_root(&user_root, &profile)?;
    let layout = WindowsStandaloneLayout::new(&user_root);
    let approval = parsed
        .restore_app
        .then(|| capture_windows_uninstall_approval(&user_root))
        .transpose()?;

    print_plan(&layout, parsed.restore_app);
    if parsed.dry_run {
        println!("{}", format_warn("Dry run. No files changed.", None));
        println!();
        return Ok(());
    }
    crate::confirm::require("self-uninstall", parsed.yes)?;

    let _channel = acquire_windows_update_lock(&package_root)?;
    validate_managed_install_identity(&package_root, &running_exe)?;
    if let Some(approval) = approval.as_ref() {
        match uninstall_windows_runtime_approved_with(
            &user_root,
            approval,
            crate::windows_process::running_package_process_ids,
            crate::windows_app::codex_package_full_name_is_installed,
            crate::windows_activation::disable_installed_runtime,
        )? {
            WindowsUninstallOutcome::NotInstalled | WindowsUninstallOutcome::Removed => {}
            WindowsUninstallOutcome::CloseRequired { process_ids } => {
                return Err(format!(
                    "close Codex before removing the Windows Runtime; finish active work, then use Ctrl+Q or the tray Quit command (running package PIDs: {})",
                    process_ids
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }

    start_windows_self_uninstall_handoff(&user_root, &package_root, std::process::id(), true)?;
    println!(
        "{}",
        format_ok(
            "CLI removal scheduled. It will finish after this command exits.",
            None
        )
    );
    println!();
    Ok(())
}

fn print_plan(layout: &WindowsStandaloneLayout, restore_runtime: bool) {
    println!("{}", format_step("Self-uninstall", None));
    println!(
        "{}",
        format_kv(
            "CLI",
            &crate::windows_system::windows_path_for_display(&layout.package_root()),
            None,
        )
    );
    println!(
        "{}",
        format_kv(
            "Launchers",
            &crate::windows_system::windows_path_for_display(&layout.bin_dir()),
            None,
        )
    );
    println!("{}", format_kv("User PATH", "Remove CLI bin entry", None));
    if restore_runtime {
        println!(
            "{}",
            format_warn("Also remove the Windows Runtime integration.", None)
        );
    } else {
        println!(
            "{}",
            format_warn("Runtime and incognito session state are preserved.", None)
        );
    }
    println!(
        "{}",
        format_warn("The Microsoft Store package is not modified.", None)
    );
}

#[doc(hidden)]
pub fn start_windows_self_uninstall_handoff(
    user_root: &Path,
    package_root: &Path,
    wait_pid: u32,
    remove_user_path: bool,
) -> Result<(), String> {
    validate_cleanup_layout(user_root, package_root)?;
    let layout = WindowsStandaloneLayout::new(user_root);
    let cache =
        incodex_core::windows_session::ensure_private_windows_dir(&user_root.join("cache"))?;
    let sequence = SCRIPT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let script = cache.join(format!("self-uninstall-{wait_pid}-{sequence}.ps1"));
    let error_log = cache.join("self-uninstall-error.log");
    let powershell =
        crate::windows_system::system_binary_path("WindowsPowerShell/v1.0/powershell.exe")?;
    let primary = powershell_path(&layout.primary_launcher())?;
    let alias = powershell_path(&layout.alias_launcher())?;
    let package = powershell_path(package_root)?;
    let packages = powershell_path(&user_root.join("packages"))?;
    let bin = powershell_path(&layout.bin_dir())?;
    let script_path = powershell_path(&script)?;
    let error_log = powershell_path(&error_log)?;
    write_private_script(&script)?;

    let mut command = Command::new(powershell);
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .env("INCODEX_SELF_UNINSTALL_PID", wait_pid.to_string())
        .env("INCODEX_SELF_UNINSTALL_PRIMARY", primary)
        .env("INCODEX_SELF_UNINSTALL_ALIAS", alias)
        .env("INCODEX_SELF_UNINSTALL_PACKAGE", package)
        .env("INCODEX_SELF_UNINSTALL_PACKAGES", packages)
        .env("INCODEX_SELF_UNINSTALL_BIN", bin)
        .env(
            "INCODEX_SELF_UNINSTALL_REMOVE_PATH",
            if remove_user_path { "1" } else { "0" },
        )
        .env("INCODEX_SELF_UNINSTALL_SCRIPT", script_path)
        .env("INCODEX_SELF_UNINSTALL_ERROR", error_log)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    if let Err(error) = command.spawn() {
        let _ = fs::remove_file(&script);
        return Err(format!(
            "could not start the Windows self-uninstall cleanup: {error}"
        ));
    }
    Ok(())
}

fn validate_cleanup_layout(user_root: &Path, package_root: &Path) -> Result<(), String> {
    if !user_root.is_absolute() || !package_root.is_absolute() {
        return Err("Windows self-uninstall paths must be absolute".to_string());
    }
    let expected = user_root.join("packages").join("standalone");
    if !package_root
        .to_string_lossy()
        .eq_ignore_ascii_case(&expected.to_string_lossy())
    {
        return Err("Windows self-uninstall package root has an invalid layout".to_string());
    }
    for (path, label) in [
        (user_root, "Windows user root"),
        (&user_root.join("bin"), "Windows CLI bin directory"),
        (&user_root.join("packages"), "Windows package directory"),
        (package_root, "Windows standalone package root"),
    ] {
        reject_reparse_ancestors(path)?;
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("cannot inspect {label}: {error}"))?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(format!("{label} is not a regular directory"));
        }
        incodex_core::windows_session::verify_private_acl(path)?;
    }
    Ok(())
}

fn write_private_script(path: &Path) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("cannot create Windows self-uninstall script: {error}"))?;
    file.write_all(CLEANUP_SCRIPT.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("cannot write Windows self-uninstall script: {error}"))?;
    incodex_core::windows_session::apply_private_windows_acl(path)?;
    incodex_core::windows_session::verify_private_acl(path)
}

fn powershell_path(path: &Path) -> Result<String, String> {
    if !path.is_absolute() {
        return Err("Windows self-uninstall path is not absolute".to_string());
    }
    let display = crate::windows_system::windows_path_for_display(path);
    if display.starts_with(r"\\?\") {
        return Err("PowerShell cannot remove this Windows path form".to_string());
    }
    Ok(display)
}
