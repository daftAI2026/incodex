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
    retire_unreadable_windows_install_state, retire_windows_update_repair_intent,
    stage_windows_install_state, transition_windows_install_state,
    transition_windows_uninstall_state, WindowsInstallPhase, WindowsInstallState,
};
use crate::windows_process::running_package_process_ids;
use crate::windows_registration::{
    read_transient_windows_debug_registration, read_windows_debug_registration,
    recover_transient_windows_debug_registration_with_restore, registration_matches_install_state,
    retire_windows_debug_registration, retire_windows_debug_registration_file,
    stage_installed_windows_debug_registration, transient_windows_debug_registration_exists,
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
    let profile = crate::windows_profile::windows_user_profile()?;
    let user_root = profile.join(".incodex");
    let _registration_gate = acquire_windows_install_state()?;
    let confirmed_app = discover_codex_package()?;
    if confirmed_app.package_full_name != app.package_full_name {
        return Err(
            "Windows install target changed since confirmation; review the plan and retry"
                .to_string(),
        );
    }
    recover_transient_windows_debug_registration_with_restore(
        &user_root,
        running_package_process_ids,
        codex_package_full_name_is_installed,
        disable_installed_runtime,
        |state| {
            WindowsInstalledRuntimeRegistration::from_install_state(state)
                .and_then(|registration| enable_installed_runtime(&registration))
        },
    )?;
    let helper = std::env::current_exe()
        .map_err(|error| format!("cannot locate the running Incodex executable: {error}"))?;
    let installed = install_windows_runtime_with_package_probe(
        WindowsInstallTarget {
            user_root: &user_root,
            package_full_name: &confirmed_app.package_full_name,
            helper_source: &helper,
        },
        running_package_process_ids,
        codex_package_full_name_is_installed,
        disable_installed_runtime,
        enable_installed_runtime,
        || discover_codex_package().map(|app| app.package_full_name),
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

struct WindowsInstallTarget<'a> {
    user_root: &'a Path,
    package_full_name: &'a str,
    helper_source: &'a Path,
}

pub fn install_windows_runtime_with<R, P, D, E>(
    user_root: &Path,
    package_full_name: &str,
    helper_source: &Path,
    running_package_processes: R,
    package_is_installed: P,
    disable: D,
    enable: E,
) -> Result<WindowsInstallState, String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
    E: FnOnce(&WindowsInstalledRuntimeRegistration) -> Result<(), String>,
{
    install_windows_runtime_with_package_probe(
        WindowsInstallTarget {
            user_root,
            package_full_name,
            helper_source,
        },
        running_package_processes,
        package_is_installed,
        disable,
        enable,
        || Ok(package_full_name.to_string()),
    )
}

pub(crate) fn install_windows_runtime_locked_with<R, P, D, E>(
    user_root: &Path,
    package_full_name: &str,
    helper_source: &Path,
    running_package_processes: R,
    package_is_installed: P,
    disable: D,
    enable: E,
) -> Result<WindowsInstallState, String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
    E: FnOnce(&WindowsInstalledRuntimeRegistration) -> Result<(), String>,
{
    install_windows_runtime_locked_with_package_probe(
        WindowsInstallTarget {
            user_root,
            package_full_name,
            helper_source,
        },
        running_package_processes,
        package_is_installed,
        disable,
        enable,
        || Ok(package_full_name.to_string()),
    )
}

fn install_windows_runtime_with_package_probe<R, P, D, E, G>(
    target: WindowsInstallTarget<'_>,
    mut running_package_processes: R,
    mut package_is_installed: P,
    mut disable: D,
    enable: E,
    package_probe: G,
) -> Result<WindowsInstallState, String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
    E: FnOnce(&WindowsInstalledRuntimeRegistration) -> Result<(), String>,
    G: FnMut() -> Result<String, String>,
{
    let _transaction = acquire_windows_install_state()?;
    let WindowsInstallTarget {
        user_root,
        package_full_name,
        helper_source,
    } = target;
    crate::windows_update_repair::prepare_interrupted_update_repair_with(
        user_root,
        package_full_name,
        &mut running_package_processes,
        &mut package_is_installed,
        &mut disable,
    )?;
    let installed = install_windows_runtime_locked_with_package_probe(
        WindowsInstallTarget {
            user_root,
            package_full_name,
            helper_source,
        },
        running_package_processes,
        package_is_installed,
        disable,
        enable,
        package_probe,
    )?;
    retire_windows_update_repair_intent(user_root, None)?;
    Ok(installed)
}

fn install_windows_runtime_locked_with_package_probe<R, P, D, E, G>(
    target: WindowsInstallTarget<'_>,
    mut running_package_processes: R,
    mut package_is_installed: P,
    mut disable: D,
    enable: E,
    mut package_probe: G,
) -> Result<WindowsInstallState, String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
    E: FnOnce(&WindowsInstalledRuntimeRegistration) -> Result<(), String>,
    G: FnMut() -> Result<String, String>,
{
    let WindowsInstallTarget {
        user_root,
        package_full_name,
        helper_source,
    } = target;
    revalidate_windows_install_generation(package_full_name, &mut package_probe)?;
    let existing = read_windows_install_state(user_root)?;
    if let Some(existing) = existing.as_ref() {
        if existing.package_full_name == package_full_name {
            return Err(format!(
                "Windows Runtime already has durable state {:?} for {}; uninstall or recover it before installing again",
                existing.phase, existing.package_full_name
            ));
        }
    }
    let running = running_package_processes(package_full_name)
        .map_err(|error| format!("cannot inspect running Windows Codex processes: {error}"))?;
    if !running.is_empty() {
        return Err(format!(
            "close Codex before installing the Windows Runtime; finish active work, then use Ctrl+Q or the tray Quit command (running package PIDs: {})",
            running
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if existing.is_some() {
        match uninstall_windows_runtime_locked_with(
            user_root,
            &mut running_package_processes,
            &mut package_is_installed,
            &mut disable,
        )? {
            WindowsUninstallOutcome::Removed | WindowsUninstallOutcome::NotInstalled => {}
            WindowsUninstallOutcome::CloseRequired { process_ids } => {
                return Err(format!(
                    "close the previous Codex Store generation before replacing its Windows Runtime registration (running package PIDs: {})",
                    process_ids
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
    }

    let runtime = publish_windows_runtime(user_root)?;
    let runtime_release = runtime
        .release_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Windows Runtime release name is not valid Unicode".to_string())?;
    let helper = publish_windows_helper(user_root, helper_source)?;
    revalidate_windows_install_generation(package_full_name, &mut package_probe)?;
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
    let running_after_enable = match running_package_processes(package_full_name) {
        Ok(running) => running,
        Err(error) => {
            return Err(recover_after_install_enable_failure(
                user_root,
                pending.epoch,
                package_full_name,
                &mut disable,
                format!(
                    "cannot prove Codex remained closed while enabling the Windows Runtime: {error}"
                ),
            ));
        }
    };
    if !running_after_enable.is_empty() {
        return Err(recover_after_install_enable_failure(
            user_root,
            pending.epoch,
            package_full_name,
            &mut disable,
            format!(
                "Codex started while the Windows Runtime registration was being enabled (running package PIDs: {})",
                running_after_enable
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }

    if let Err(error) = revalidate_windows_install_generation(package_full_name, &mut package_probe)
    {
        return Err(recover_after_install_enable_failure(
            user_root,
            pending.epoch,
            package_full_name,
            &mut disable,
            format!("Windows Store generation changed while enabling the Runtime: {error}"),
        ));
    }

    let enabled = match transition_windows_install_state(
        user_root,
        pending.epoch,
        WindowsInstallPhase::EnabledUnobserved,
    ) {
        Ok(enabled) => enabled,
        Err(error) => {
            let recovery = transition_windows_install_state(
                user_root,
                pending.epoch,
                WindowsInstallPhase::RecoveryRequired,
            );
            return Err(join_recovery_error(
                format!(
                    "Windows Runtime was enabled but durable state could not be committed: {error}"
                ),
                recovery,
            ));
        }
    };

    if let Err(error) = revalidate_windows_install_generation(package_full_name, &mut package_probe)
    {
        return Err(recover_after_install_enable_failure(
            user_root,
            enabled.epoch,
            package_full_name,
            &mut disable,
            format!("Windows Store generation changed after enabling the Runtime: {error}"),
        ));
    }

    Ok(enabled)
}

fn revalidate_windows_install_generation<G>(
    expected_package_full_name: &str,
    package_probe: &mut G,
) -> Result<(), String>
where
    G: FnMut() -> Result<String, String>,
{
    let current_package_full_name = package_probe()?;
    if current_package_full_name == expected_package_full_name {
        return Ok(());
    }
    Err(format!(
        "Windows install target changed during mutation: expected {expected_package_full_name}, found {current_package_full_name}"
    ))
}

fn recover_after_install_enable_failure<D>(
    user_root: &Path,
    expected_epoch: u64,
    package_full_name: &str,
    disable: &mut D,
    error: String,
) -> String
where
    D: FnMut(&str) -> Result<(), String>,
{
    let rollback = disable(package_full_name);
    let recovery = transition_windows_install_state(
        user_root,
        expected_epoch,
        WindowsInstallPhase::RecoveryRequired,
    );
    join_recovery_error(join_registration_rollback_error(error, rollback), recovery)
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

fn join_registration_rollback_error(error: String, rollback: Result<(), String>) -> String {
    match rollback {
        Ok(()) => format!("{error}; debugger registration rollback completed"),
        Err(rollback_error) => {
            format!("{error}; debugger registration rollback failed: {rollback_error}")
        }
    }
}

pub fn run_uninstall(parsed: &ParsedCli) -> Result<(), String> {
    reject_windows_target_selectors(parsed)?;
    let profile = crate::windows_profile::windows_user_profile()?;
    let user_root = profile.join(".incodex");
    let durable_state = read_windows_install_state_for_uninstall(&user_root);
    let registration_evidence = read_windows_debug_registration(&user_root);
    let transient_registration_evidence = read_transient_windows_debug_registration(&user_root);
    let approval = WindowsUninstallApproval::from_snapshots(
        &durable_state,
        &registration_evidence,
        &transient_registration_evidence,
    );
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
    if let Err(error) = &transient_registration_evidence {
        println!(
            "{}",
            format_warn(
                &format!(
                    "Transient debugger registration recovery evidence is unreadable: {error}"
                ),
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
    match uninstall_windows_runtime_approved_with_restore(
        &user_root,
        &approval,
        running_package_process_ids,
        codex_package_full_name_is_installed,
        disable_installed_runtime,
        |state| {
            WindowsInstalledRuntimeRegistration::from_install_state(state)
                .and_then(|registration| enable_installed_runtime(&registration))
        },
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
                "close Codex to finish uninstalling the Windows Runtime; finish active work, then use Ctrl+Q or the tray Quit command (running package PIDs: {})",
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
    transient_package_full_name: Option<String>,
    transient_registration_id: Option<String>,
}

impl WindowsUninstallApproval {
    fn from_snapshots(
        state: &Result<Option<WindowsInstallState>, String>,
        evidence: &Result<Option<WindowsDebugRegistrationEvidence>, String>,
        transient_evidence: &Result<Option<WindowsDebugRegistrationEvidence>, String>,
    ) -> Self {
        let primary = match (state, evidence) {
            (Ok(Some(state)), _) => Some((&state.package_full_name, &state.registration_id)),
            (_, Ok(Some(evidence))) => {
                Some((&evidence.package_full_name, &evidence.registration_id))
            }
            _ => None,
        };
        let transient = transient_evidence
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
            .map(|evidence| (&evidence.package_full_name, &evidence.registration_id));
        let displayed = primary.or(transient);
        Self {
            package_full_name: displayed.map(|(package, _)| package.clone()),
            registration_id: displayed.map(|(_, registration)| registration.clone()),
            transient_package_full_name: transient.map(|(package, _)| package.clone()),
            transient_registration_id: transient.map(|(_, registration)| registration.clone()),
        }
    }
}

pub fn capture_windows_uninstall_approval(
    user_root: &Path,
) -> Result<WindowsUninstallApproval, String> {
    let state = read_windows_install_state_for_uninstall(user_root);
    let evidence = read_windows_debug_registration(user_root);
    let transient_evidence = read_transient_windows_debug_registration(user_root);
    let approval = WindowsUninstallApproval::from_snapshots(&state, &evidence, &transient_evidence);
    if approval.package_full_name.is_some()
        || (state.is_ok() && evidence.is_ok() && transient_evidence.is_ok())
    {
        return Ok(approval);
    }
    let mut errors = [state.err(), evidence.err(), transient_evidence.err()]
        .into_iter()
        .flatten();
    let Some(first) = errors.next() else {
        return Ok(approval);
    };
    let detail = errors.fold(first, |detail, error| format!("{detail}; {error}"));
    Err(detail)
}

pub fn uninstall_windows_runtime_approved_with<R, P, D>(
    user_root: &Path,
    approved: &WindowsUninstallApproval,
    running_package_processes: R,
    package_is_installed: P,
    disable: D,
) -> Result<WindowsUninstallOutcome, String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
{
    uninstall_windows_runtime_approved_with_restore(
        user_root,
        approved,
        running_package_processes,
        package_is_installed,
        disable,
        |_| {
            Err(
                "an installed Windows registration cannot be restored through this uninstall path"
                    .to_string(),
            )
        },
    )
}

pub fn uninstall_windows_runtime_approved_with_restore<R, P, D, E>(
    user_root: &Path,
    approved: &WindowsUninstallApproval,
    running_package_processes: R,
    package_is_installed: P,
    disable: D,
    enable_installed: E,
) -> Result<WindowsUninstallOutcome, String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
    E: FnMut(&WindowsInstallState) -> Result<(), String>,
{
    let _transaction = acquire_windows_install_state()?;
    let current_state = read_windows_install_state_for_uninstall(user_root);
    let current_evidence = read_windows_debug_registration(user_root);
    let current_transient_evidence = read_transient_windows_debug_registration(user_root);
    let current = WindowsUninstallApproval::from_snapshots(
        &current_state,
        &current_evidence,
        &current_transient_evidence,
    );
    if current != *approved {
        return Err(
            "Windows uninstall target changed since confirmation; review the plan and retry"
                .to_string(),
        );
    }
    let mut running_package_processes = running_package_processes;
    let mut package_is_installed = package_is_installed;
    let mut disable = disable;
    if transient_windows_debug_registration_exists(user_root)? {
        recover_transient_windows_debug_registration_with_restore(
            user_root,
            &mut running_package_processes,
            &mut package_is_installed,
            &mut disable,
            enable_installed,
        )?;
    }
    let outcome = uninstall_windows_runtime_locked_with(
        user_root,
        &mut running_package_processes,
        &mut package_is_installed,
        &mut disable,
    )?;
    if matches!(
        outcome,
        WindowsUninstallOutcome::Removed | WindowsUninstallOutcome::NotInstalled
    ) {
        retire_windows_update_repair_intent(user_root, None)?;
    }
    Ok(outcome)
}

pub fn uninstall_windows_runtime_with<R, P, D>(
    user_root: &Path,
    running_package_processes: R,
    package_is_installed: P,
    disable: D,
) -> Result<WindowsUninstallOutcome, String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
{
    uninstall_windows_runtime_with_restore(
        user_root,
        running_package_processes,
        package_is_installed,
        disable,
        |_| {
            Err(
                "an installed Windows registration cannot be restored through this uninstall path"
                    .to_string(),
            )
        },
    )
}

pub fn uninstall_windows_runtime_with_restore<R, P, D, E>(
    user_root: &Path,
    running_package_processes: R,
    package_is_installed: P,
    disable: D,
    enable_installed: E,
) -> Result<WindowsUninstallOutcome, String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
    E: FnMut(&WindowsInstallState) -> Result<(), String>,
{
    let _transaction = acquire_windows_install_state()?;
    let mut running_package_processes = running_package_processes;
    let mut package_is_installed = package_is_installed;
    let mut disable = disable;
    if transient_windows_debug_registration_exists(user_root)? {
        recover_transient_windows_debug_registration_with_restore(
            user_root,
            &mut running_package_processes,
            &mut package_is_installed,
            &mut disable,
            enable_installed,
        )?;
    }
    let outcome = uninstall_windows_runtime_locked_with(
        user_root,
        &mut running_package_processes,
        &mut package_is_installed,
        &mut disable,
    )?;
    if matches!(
        outcome,
        WindowsUninstallOutcome::Removed | WindowsUninstallOutcome::NotInstalled
    ) {
        retire_windows_update_repair_intent(user_root, None)?;
    }
    Ok(outcome)
}

pub(crate) fn uninstall_windows_runtime_locked_with<R, P, D>(
    user_root: &Path,
    running_package_processes: &mut R,
    package_is_installed: &mut P,
    disable: &mut D,
) -> Result<WindowsUninstallOutcome, String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
{
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
    let running_after_disable = match running_package_processes(&pending.package_full_name) {
        Ok(running) => running,
        Err(error) => {
            let recovery = transition_windows_uninstall_state(
                user_root,
                pending.epoch,
                WindowsInstallPhase::RecoveryRequired,
            );
            return Err(join_recovery_error(
                format!(
                    "cannot prove Codex remained closed while disabling the Windows Runtime: {error}"
                ),
                recovery,
            ));
        }
    };
    if !running_after_disable.is_empty() {
        transition_windows_uninstall_state(
            user_root,
            pending.epoch,
            WindowsInstallPhase::RecoveryRequired,
        )?;
        return Ok(WindowsUninstallOutcome::CloseRequired {
            process_ids: running_after_disable,
        });
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
    running_package_processes: &mut R,
    package_is_installed: &mut P,
    disable: &mut D,
) -> Result<WindowsUninstallOutcome, String>
where
    R: FnMut(&str) -> Result<Vec<u32>, std::io::Error>,
    P: FnMut(&str) -> Result<bool, String>,
    D: FnMut(&str) -> Result<(), String>,
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
    let running_after_disable =
        running_package_processes(&evidence.package_full_name).map_err(|error| {
            format!(
                "cannot prove Codex remained closed while disabling the Windows Runtime: {error}"
            )
        })?;
    if !running_after_disable.is_empty() {
        return Ok(WindowsUninstallOutcome::CloseRequired {
            process_ids: running_after_disable,
        });
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
    let mut lines = vec![
        format_step("Uninstall", None),
        format_kv("Package", package, None),
        format_kv("Registration", registration, None),
    ];
    if let (Some(transient_package), Some(transient_registration)) = (
        approval.transient_package_full_name.as_deref(),
        approval.transient_registration_id.as_deref(),
    ) {
        if transient_package != package || transient_registration != registration {
            lines.push(format_kv(
                "Transient",
                &format!("{transient_package} / {transient_registration}"),
                None,
            ));
        }
    }
    lines.push(format_kv("App", &executable, None));
    lines.push(format_warn(
        "The Microsoft Store package is not modified.",
        None,
    ));
    lines.join("\n")
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

        let approval =
            WindowsUninstallApproval::from_snapshots(&Ok(None), &Ok(Some(evidence)), &Ok(None));
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

    #[test]
    fn install_retains_recovery_when_store_generation_changes_during_mutation() {
        let user_root = std::env::temp_dir().join(format!(
            "incodex-windows-install-generation-race-{}",
            std::process::id()
        ));
        let helper = std::env::current_exe().expect("test helper source");
        let expected_package = "OpenAI.Codex_1.2.3.4_x64__publisher";
        let current_package = "OpenAI.Codex_1.2.3.5_x64__publisher";
        let mut probes = 0;
        let mut disable_calls = 0;

        let error = install_windows_runtime_with_package_probe(
            WindowsInstallTarget {
                user_root: &user_root,
                package_full_name: expected_package,
                helper_source: &helper,
            },
            |_| Ok(Vec::new()),
            |_| Ok(false),
            |package| {
                disable_calls += 1;
                assert_eq!(package, expected_package);
                Ok(())
            },
            |_| Ok(()),
            || {
                probes += 1;
                Ok(if probes < 3 {
                    expected_package.to_string()
                } else {
                    current_package.to_string()
                })
            },
        )
        .expect_err("Store generation changes must not commit obsolete integration");

        assert!(error.contains("RecoveryRequired"), "{error}");
        assert!(error.contains("changed during mutation"), "{error}");
        assert_eq!(disable_calls, 1);
        let retained = read_windows_install_state(&user_root)
            .expect("read retained generation-race state")
            .expect("generation-race state retained");
        assert_eq!(retained.package_full_name, expected_package);
        assert_eq!(retained.phase, WindowsInstallPhase::RecoveryRequired);
        assert!(user_root.join("windows-registration.json").is_file());
        std::fs::remove_dir_all(user_root).expect("remove generation-race fixture");
    }
}
