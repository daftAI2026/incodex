use std::path::Path;

use incodex_core::{format_kv, format_ok, format_step, format_warn};

use crate::parse::ParsedCli;
use crate::windows_activation::{
    disable_installed_runtime, enable_installed_runtime, WindowsInstalledRuntimeRegistration,
};
use crate::windows_app::{
    codex_package_full_name_is_installed, discover_codex_package, WindowsCodexApp,
};
use crate::windows_helper::publish_windows_helper;
use crate::windows_install_state::{
    acquire_windows_install_state, read_windows_install_state,
    read_windows_install_state_for_uninstall, retire_disabled_windows_install_state,
    retire_unreadable_windows_install_state, stage_windows_install_state,
    transition_windows_install_state, transition_windows_uninstall_state, WindowsInstallPhase,
    WindowsInstallState,
};
use crate::windows_process::running_package_process_ids;
use crate::windows_quiescence::request_official_package_exit_and_wait;
use crate::windows_registration::{
    read_windows_debug_registration, recover_transient_windows_debug_registration_with,
    registration_matches_install_state, retire_windows_debug_registration,
    retire_windows_debug_registration_file, stage_installed_windows_debug_registration,
    WindowsDebugRegistrationEvidence,
};
use crate::windows_runtime::publish_windows_runtime;
use crate::windows_system::windows_path_for_display;

pub fn run_install(parsed: &ParsedCli) -> Result<(), String> {
    reject_windows_target_selectors(parsed)?;
    let app = discover_codex_package()?;
    print_plan("Install", &app);
    if parsed.dry_run {
        println!("{}", format_warn("Dry run. No files changed.", None));
        println!();
        return Ok(());
    }
    crate::confirm::require("install", parsed.yes)?;
    request_official_package_exit_and_wait(&app.package_full_name, None)?;
    let profile = crate::windows_profile::windows_user_profile()?;
    let user_root = profile.join(".incodex");
    let _registration_gate = acquire_windows_install_state()?;
    recover_transient_windows_debug_registration_with(
        &user_root,
        codex_package_full_name_is_installed,
        disable_installed_runtime,
    )?;
    let helper = std::env::current_exe()
        .map_err(|error| format!("cannot locate the running Incodex executable: {error}"))?;
    let installed = install_windows_runtime_with(
        &user_root,
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
    let _transaction = acquire_windows_install_state()?;
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
    let registration = match WindowsInstalledRuntimeRegistration::from_install_state(&pending) {
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
    if let Err(error) = stage_installed_windows_debug_registration(user_root, &pending) {
        let recovery = transition_windows_install_state(
            user_root,
            pending.epoch,
            WindowsInstallPhase::RecoveryRequired,
        );
        return Err(join_recovery_error(error, recovery));
    }
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
            windows_path_for_display(&state.state_path)
        ),
        Err(recovery_error) => {
            format!("{error}; Windows install recovery state is unproven: {recovery_error}")
        }
    }
}

pub fn run_uninstall(parsed: &ParsedCli) -> Result<(), String> {
    reject_windows_target_selectors(parsed)?;
    let profile = crate::windows_profile::windows_user_profile()?;
    let user_root = profile.join(".incodex");
    let durable_state = read_windows_install_state_for_uninstall(&user_root);
    let registration_evidence = read_windows_debug_registration(&user_root);
    let approval = WindowsUninstallApproval::from_snapshots(&durable_state, &registration_evidence);
    let discovered_app = discover_codex_package();
    print_uninstall_plan(discovered_app.as_ref().ok(), &approval);
    if let Err(error) = &durable_state {
        println!(
            "{}",
            format_warn(
                &format!("Primary install state needs registration recovery: {error}"),
                None
            )
        );
    }
    if let Err(error) = &registration_evidence {
        println!(
            "{}",
            format_warn(
                &format!("Debugger registration recovery evidence is unreadable: {error}"),
                None
            )
        );
    }
    if let Err(error) = &discovered_app {
        println!(
            "{}",
            format_warn(
                &format!("The Microsoft Store package is unavailable: {error}"),
                None
            )
        );
    }
    if parsed.dry_run {
        println!("{}", format_warn("Dry run. No files changed.", None));
        println!();
        return Ok(());
    }
    crate::confirm::require("uninstall", parsed.yes)?;
    if let Some(package_full_name) = approval.package_full_name.as_deref() {
        request_official_package_exit_and_wait(
            package_full_name,
            approval.registration_id.as_deref(),
        )?;
    }
    match uninstall_windows_runtime_approved_with(
        &user_root,
        &approval,
        running_package_process_ids,
        codex_package_full_name_is_installed,
        disable_installed_runtime,
    )? {
        WindowsUninstallOutcome::NotInstalled => {
            println!("{}", format_ok("Incodex is not installed.", None));
        }
        WindowsUninstallOutcome::Removed => {
            println!(
                "{}",
                format_ok("Uninstalled Windows Runtime integration.", None)
            );
        }
        WindowsUninstallOutcome::CloseRequired { process_ids } => {
            return Err(format!(
                "close Codex to finish uninstalling the Windows Runtime (running package PIDs: {})",
                process_ids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    println!();
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsUninstallOutcome {
    NotInstalled,
    CloseRequired { process_ids: Vec<u32> },
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsUninstallApproval {
    package_full_name: Option<String>,
    registration_id: Option<String>,
}

impl WindowsUninstallApproval {
    fn from_snapshots(
        state: &Result<Option<WindowsInstallState>, String>,
        evidence: &Result<Option<WindowsDebugRegistrationEvidence>, String>,
    ) -> Self {
        if let Ok(Some(state)) = state {
            return Self {
                package_full_name: Some(state.package_full_name.clone()),
                registration_id: Some(state.registration_id.clone()),
            };
        }
        if let Ok(Some(evidence)) = evidence {
            return Self {
                package_full_name: Some(evidence.package_full_name.clone()),
                registration_id: Some(evidence.registration_id.clone()),
            };
        }
        Self {
            package_full_name: None,
            registration_id: None,
        }
    }
}

pub fn capture_windows_uninstall_approval(
    user_root: &Path,
) -> Result<WindowsUninstallApproval, String> {
    let state = read_windows_install_state_for_uninstall(user_root);
    let evidence = read_windows_debug_registration(user_root);
    let approval = WindowsUninstallApproval::from_snapshots(&state, &evidence);
    if approval.package_full_name.is_some() || (state.is_ok() && evidence.is_ok()) {
        return Ok(approval);
    }
    match (state, evidence) {
        (Err(state_error), Err(evidence_error)) => Err(format!(
            "{state_error}; Windows debugger registration evidence is also unreadable: {evidence_error}"
        )),
        (Err(error), _) | (_, Err(error)) => Err(error),
        _ => Ok(approval),
    }
}

pub fn uninstall_windows_runtime_approved_with<R, P, D>(
    user_root: &Path,
    approved: &WindowsUninstallApproval,
    running_package_processes: R,
    package_is_installed: P,
    disable: D,
) -> Result<WindowsUninstallOutcome, String>
where
    R: FnOnce(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnOnce(&str) -> Result<bool, String>,
    D: FnOnce(&str) -> Result<(), String>,
{
    let _transaction = acquire_windows_install_state()?;
    let current_state = read_windows_install_state_for_uninstall(user_root);
    let current_evidence = read_windows_debug_registration(user_root);
    let current = WindowsUninstallApproval::from_snapshots(&current_state, &current_evidence);
    if current != *approved {
        return Err(
            "Windows uninstall target changed since confirmation; review the plan and retry"
                .to_string(),
        );
    }
    uninstall_windows_runtime_with(
        user_root,
        running_package_processes,
        package_is_installed,
        disable,
    )
}

pub fn uninstall_windows_runtime_with<R, P, D>(
    user_root: &Path,
    running_package_processes: R,
    package_is_installed: P,
    disable: D,
) -> Result<WindowsUninstallOutcome, String>
where
    R: FnOnce(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnOnce(&str) -> Result<bool, String>,
    D: FnOnce(&str) -> Result<(), String>,
{
    let _transaction = acquire_windows_install_state()?;
    let state_result = read_windows_install_state_for_uninstall(user_root);
    let evidence_result = read_windows_debug_registration(user_root);
    let mut state = match state_result {
        Ok(Some(state)) => {
            match &evidence_result {
                Ok(Some(evidence)) if !registration_matches_install_state(evidence, &state) => {
                    return Err(
                        "Windows install state and debugger registration evidence disagree"
                            .to_string(),
                    )
                }
                Ok(_) | Err(_) => {}
            }
            state
        }
        Ok(None) => {
            return uninstall_windows_registration_without_state(
                user_root,
                evidence_result?,
                false,
                running_package_processes,
                package_is_installed,
                disable,
            )
        }
        Err(state_error) => {
            let evidence = evidence_result.map_err(|evidence_error| {
                format!(
                    "{state_error}; Windows debugger registration evidence is also unreadable: {evidence_error}"
                )
            })?;
            let Some(evidence) = evidence else {
                return Err(state_error);
            };
            return uninstall_windows_registration_without_state(
                user_root,
                Some(evidence),
                true,
                running_package_processes,
                package_is_installed,
                disable,
            );
        }
    };
    state = match state.phase {
        WindowsInstallPhase::Staged => transition_windows_uninstall_state(
            user_root,
            state.epoch,
            WindowsInstallPhase::Disabled,
        )?,
        WindowsInstallPhase::EnablePending => transition_windows_uninstall_state(
            user_root,
            state.epoch,
            WindowsInstallPhase::RecoveryRequired,
        )?,
        WindowsInstallPhase::EnabledUnobserved | WindowsInstallPhase::EnabledObserved => {
            transition_windows_uninstall_state(
                user_root,
                state.epoch,
                WindowsInstallPhase::DisableRequested,
            )?
        }
        WindowsInstallPhase::DisableRequested
        | WindowsInstallPhase::DisablePending
        | WindowsInstallPhase::Disabled
        | WindowsInstallPhase::RecoveryRequired => state,
    };
    if state.phase == WindowsInstallPhase::Disabled {
        retire_windows_debug_registration_file(user_root)?;
        retire_disabled_windows_install_state(user_root, state.epoch)?;
        return Ok(WindowsUninstallOutcome::Removed);
    }

    let running = running_package_processes(&state.package_full_name)
        .map_err(|error| format!("cannot inspect running Windows Codex processes: {error}"))?;
    if !running.is_empty() {
        return Ok(WindowsUninstallOutcome::CloseRequired {
            process_ids: running,
        });
    }
    let pending = if state.phase == WindowsInstallPhase::DisablePending {
        state
    } else {
        transition_windows_uninstall_state(
            user_root,
            state.epoch,
            WindowsInstallPhase::DisablePending,
        )?
    };
    let package_is_installed = match package_is_installed(&pending.package_full_name) {
        Ok(installed) => installed,
        Err(error) => {
            let recovery = transition_windows_uninstall_state(
                user_root,
                pending.epoch,
                WindowsInstallPhase::RecoveryRequired,
            );
            return Err(join_recovery_error(error, recovery));
        }
    };
    if package_is_installed {
        if let Err(error) = disable(&pending.package_full_name) {
            let recovery = transition_windows_uninstall_state(
                user_root,
                pending.epoch,
                WindowsInstallPhase::RecoveryRequired,
            );
            return Err(join_recovery_error(error, recovery));
        }
    }
    let disabled = match transition_windows_uninstall_state(
        user_root,
        pending.epoch,
        WindowsInstallPhase::Disabled,
    ) {
        Ok(disabled) => disabled,
        Err(error) => {
            let recovery = transition_windows_uninstall_state(
                user_root,
                pending.epoch,
                WindowsInstallPhase::RecoveryRequired,
            );
            return Err(join_recovery_error(
                format!(
                    "Windows Runtime was disabled but durable state could not be committed: {error}"
                ),
                recovery,
            ));
        }
    };
    retire_windows_debug_registration_file(user_root)?;
    retire_disabled_windows_install_state(user_root, disabled.epoch)?;
    Ok(WindowsUninstallOutcome::Removed)
}

fn uninstall_windows_registration_without_state<R, P, D>(
    user_root: &Path,
    evidence: Option<WindowsDebugRegistrationEvidence>,
    retire_unreadable_state: bool,
    running_package_processes: R,
    package_is_installed: P,
    disable: D,
) -> Result<WindowsUninstallOutcome, String>
where
    R: FnOnce(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnOnce(&str) -> Result<bool, String>,
    D: FnOnce(&str) -> Result<(), String>,
{
    let Some(evidence) = evidence else {
        return Ok(WindowsUninstallOutcome::NotInstalled);
    };
    let running = running_package_processes(&evidence.package_full_name)
        .map_err(|error| format!("cannot inspect running Windows Codex processes: {error}"))?;
    if !running.is_empty() {
        return Ok(WindowsUninstallOutcome::CloseRequired {
            process_ids: running,
        });
    }
    if package_is_installed(&evidence.package_full_name)? {
        disable(&evidence.package_full_name)?;
    }
    if retire_unreadable_state {
        retire_unreadable_windows_install_state(user_root)?;
    }
    retire_windows_debug_registration(user_root, &evidence.registration_id)?;
    Ok(WindowsUninstallOutcome::Removed)
}

fn print_plan(action: &str, app: &WindowsCodexApp) {
    println!("{}", format_step(action, None));
    println!("{}", format_kv("Package", &app.package_full_name, None));
    println!(
        "{}",
        format_kv("App", &windows_path_for_display(&app.executable), None)
    );
    println!(
        "{}",
        format_warn("The Microsoft Store package is not modified.", None)
    );
}

fn print_uninstall_plan(app: Option<&WindowsCodexApp>, approval: &WindowsUninstallApproval) {
    println!("{}", format_uninstall_plan(app, approval));
}

fn format_uninstall_plan(
    app: Option<&WindowsCodexApp>,
    approval: &WindowsUninstallApproval,
) -> String {
    let package = uninstall_plan_package(app, approval);
    let registration = approval
        .registration_id
        .as_deref()
        .unwrap_or("Not recorded");
    let executable = app
        .filter(|app| app.package_full_name == package)
        .map(|app| windows_path_for_display(&app.executable))
        .unwrap_or_else(|| "Unavailable".to_string());
    [
        format_step("Uninstall", None),
        format_kv("Package", package, None),
        format_kv("Registration", registration, None),
        format_kv("App", &executable, None),
        format_warn("The Microsoft Store package is not modified.", None),
    ]
    .join("\n")
}

fn uninstall_plan_package<'a>(
    app: Option<&'a WindowsCodexApp>,
    approval: &'a WindowsUninstallApproval,
) -> &'a str {
    approval
        .package_full_name
        .as_deref()
        .or_else(|| app.map(|app| app.package_full_name.as_str()))
        .unwrap_or("Not discovered")
}

fn reject_windows_target_selectors(parsed: &ParsedCli) -> Result<(), String> {
    if parsed.clone || parsed.app.is_some() {
        return Err("--clone and --app are not supported on Windows".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows_registration::WindowsDebugRegistrationKind;
    use std::path::PathBuf;

    #[test]
    fn uninstall_plan_prefers_independent_recovery_evidence() {
        let evidence = WindowsDebugRegistrationEvidence {
            schema_version: 1,
            registration_id: "0123456789abcdef0123456789abcdef".to_string(),
            kind: WindowsDebugRegistrationKind::Installed,
            package_full_name: "OpenAI.Codex_1.2.3.4_x64__publisher".to_string(),
            helper_path: PathBuf::from(
                r"C:\Users\test\.incodex\windows\helpers\fixture\incodex-helper.exe",
            ),
            helper_sha256: "0".repeat(64),
            state_path: PathBuf::from(r"C:\Users\test\.incodex\windows-registration.json"),
        };
        let discovered = WindowsCodexApp {
            package_full_name: "OpenAI.Codex_1.2.3.5_x64__publisher".to_string(),
            app_user_model_id: "OpenAI.Codex_publisher!App".to_string(),
            install_location: PathBuf::from(r"C:\Program Files\WindowsApps\current"),
            executable: PathBuf::from(r"C:\Program Files\WindowsApps\current\ChatGPT.exe"),
            architecture: "X64".to_string(),
        };

        let approval = WindowsUninstallApproval::from_snapshots(&Ok(None), &Ok(Some(evidence)));
        assert_eq!(
            uninstall_plan_package(Some(&discovered), &approval),
            "OpenAI.Codex_1.2.3.4_x64__publisher"
        );
        let plan = format_uninstall_plan(Some(&discovered), &approval);
        assert!(
            plan.contains("Registration 0123456789abcdef0123456789abcdef"),
            "{plan}"
        );
    }
}
