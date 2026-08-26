use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use incodex_core::{format_kv, format_ok, format_step, format_warn};

use crate::parse::ParsedCli;
use crate::windows_activation::{enable_installed_runtime, WindowsInstalledRuntimeRegistration};
use crate::windows_app::{discover_codex_package, WindowsCodexApp};
use crate::windows_helper::publish_windows_helper;
use crate::windows_install_state::{
    read_windows_install_state, stage_windows_install_state, transition_windows_install_state,
    WindowsInstallPhase, WindowsInstallState,
};
use crate::windows_process::running_package_process_ids;
use crate::windows_runtime::publish_windows_runtime;

pub fn run_install(parsed: &ParsedCli) -> Result<(), String> {
    let app = discover_codex_package()?;
    print_plan("Install", &app);
    if parsed.dry_run {
        println!("{}", format_warn("Dry run. No files changed.", None));
        println!();
        return Ok(());
    }
    crate::confirm::require("install", parsed.yes)?;
    let profile = crate::windows_profile::windows_user_profile()?;
    let helper = std::env::current_exe()
        .map_err(|error| format!("cannot locate the running Incodex executable: {error}"))?;
    let installed = install_windows_runtime_with(
        &profile.join(".incodex"),
        &app.package_full_name,
        &helper,
        running_package_process_ids,
        enable_installed_runtime,
    )?;
    println!(
        "{}",
        format_ok(
            "Installed. Reopen Codex to load the hat-glasses control.",
            None
        )
    );
    println!(
        "{}",
        format_kv("Registration", &installed.registration_id, None)
    );
    println!();
    Ok(())
}

pub fn install_windows_runtime_with<R, E>(
    user_root: &Path,
    package_full_name: &str,
    helper_source: &Path,
    running_package_processes: R,
    enable: E,
) -> Result<WindowsInstallState, String>
where
    R: FnOnce(&str) -> Result<Vec<u32>, std::io::Error>,
    E: FnOnce(&WindowsInstalledRuntimeRegistration) -> Result<(), String>,
{
    if let Some(existing) = read_windows_install_state(user_root)? {
        return Err(format!(
            "Windows Runtime already has durable state {:?} for {}; uninstall or recover it before installing again",
            existing.phase, existing.package_full_name
        ));
    }
    let running = running_package_processes(package_full_name)
        .map_err(|error| format!("cannot inspect running Windows Codex processes: {error}"))?;
    if !running.is_empty() {
        return Err(format!(
            "close Codex before installing the Windows Runtime (running package PIDs: {})",
            running
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let runtime = publish_windows_runtime(user_root)?;
    let runtime_release = runtime
        .release_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Windows Runtime release name is not valid Unicode".to_string())?;
    let helper = publish_windows_helper(user_root, helper_source)?;
    let staged = stage_windows_install_state(
        user_root,
        package_full_name,
        &helper.executable,
        runtime_release,
    )?;
    let pending = transition_windows_install_state(
        user_root,
        staged.epoch,
        WindowsInstallPhase::EnablePending,
    )?;
    let bootstrap = runtime.release_dir.join("incodex-windows-bootstrap.cjs");
    let environment = BTreeMap::from([
        (
            "NODE_OPTIONS".to_string(),
            OsString::from(format!("--require=\"{}\"", bootstrap.display())),
        ),
        (
            "INCODEX_WINDOWS_REGISTRATION_ID".to_string(),
            OsString::from(&pending.registration_id),
        ),
        (
            "INCODEX_WINDOWS_PACKAGE_FULL_NAME".to_string(),
            OsString::from(package_full_name),
        ),
        (
            "INCODEX_WINDOWS_STATE_PATH".to_string(),
            pending.state_path.as_os_str().to_os_string(),
        ),
        (
            "INCODEX_WINDOWS_HELPER".to_string(),
            helper.executable.as_os_str().to_os_string(),
        ),
    ]);
    let registration = match WindowsInstalledRuntimeRegistration::new(
        package_full_name,
        &helper.executable,
        &pending.state_path,
        environment,
    ) {
        Ok(registration) => registration,
        Err(error) => {
            let recovery = transition_windows_install_state(
                user_root,
                pending.epoch,
                WindowsInstallPhase::RecoveryRequired,
            );
            return Err(join_recovery_error(error, recovery));
        }
    };
    if let Err(error) = enable(&registration) {
        let recovery = transition_windows_install_state(
            user_root,
            pending.epoch,
            WindowsInstallPhase::RecoveryRequired,
        );
        return Err(join_recovery_error(error, recovery));
    }
    match transition_windows_install_state(
        user_root,
        pending.epoch,
        WindowsInstallPhase::EnabledUnobserved,
    ) {
        Ok(enabled) => Ok(enabled),
        Err(error) => {
            let recovery = transition_windows_install_state(
                user_root,
                pending.epoch,
                WindowsInstallPhase::RecoveryRequired,
            );
            Err(join_recovery_error(
                format!(
                    "Windows Runtime was enabled but durable state could not be committed: {error}"
                ),
                recovery,
            ))
        }
    }
}

fn join_recovery_error(error: String, recovery: Result<WindowsInstallState, String>) -> String {
    match recovery {
        Ok(state) => format!(
            "{error}; Windows install entered {:?} at {}",
            state.phase,
            state.state_path.display()
        ),
        Err(recovery_error) => {
            format!("{error}; Windows install recovery state is unproven: {recovery_error}")
        }
    }
}

pub fn run_uninstall(parsed: &ParsedCli) -> Result<(), String> {
    let app = discover_codex_package()?;
    print_plan("Uninstall", &app);
    if parsed.dry_run {
        println!("{}", format_warn("Dry run. No files changed.", None));
        println!();
        return Ok(());
    }
    crate::confirm::require("uninstall", parsed.yes)?;
    Err("Windows uninstall is not implemented yet; no files changed".to_string())
}

fn print_plan(action: &str, app: &WindowsCodexApp) {
    println!("{}", format_step(action, None));
    println!("{}", format_kv("Package", &app.package_full_name, None));
    println!(
        "{}",
        format_kv("App", &app.executable.display().to_string(), None)
    );
    println!(
        "{}",
        format_warn("The Microsoft Store package is not modified.", None)
    );
}
