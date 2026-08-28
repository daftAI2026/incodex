use std::path::PathBuf;

use serde::Serialize;

use crate::diagnosis_presentation::STATUS_PROGRESS_MESSAGE;
use crate::parse::ParsedCli;
use crate::spinner::Spinner;
use crate::windows_app::{discover_codex_package, WindowsCodexApp};
use crate::windows_install_state::{read_windows_install_state, WindowsInstallPhase};
use crate::windows_registration::{
    read_windows_debug_registration, registration_matches_install_state,
    WindowsDebugRegistrationEvidence, WindowsDebugRegistrationKind,
};
use crate::windows_runtime::verify_installed_windows_runtime;
use crate::windows_system::windows_path_for_display;
use crate::CliFailure;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowsStatus {
    platform: &'static str,
    #[serde(flatten)]
    package: WindowsPackageStatus,
    integration: WindowsIntegrationStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WindowsIntegrationStatus {
    pub(crate) installed: bool,
    pub(crate) phase: Option<WindowsInstallPhase>,
    pub(crate) desired_enabled: bool,
    pub(crate) package_full_name: Option<String>,
    pub(crate) runtime_release: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) health_issues: Vec<String>,
}

impl WindowsIntegrationStatus {
    pub(crate) fn inspect(
        user_root: &std::path::Path,
        current_package_full_name: Option<&str>,
    ) -> Result<Self, String> {
        let registration = read_windows_debug_registration(user_root);
        let state = match read_windows_install_state(user_root) {
            Ok(Some(state)) => state,
            Ok(None) => return Ok(Self::without_install_state(registration, None)),
            Err(error) => {
                return Ok(Self::without_install_state(registration, Some(error)));
            }
        };
        let enabled = matches!(
            state.phase,
            WindowsInstallPhase::EnabledUnobserved | WindowsInstallPhase::EnabledObserved
        ) && state.desired_enabled();
        let mut health_issues = Vec::new();
        if enabled {
            if current_package_full_name != Some(state.package_full_name.as_str()) {
                health_issues.push(
                    "Installed integration targets a different Store package generation."
                        .to_string(),
                );
            }
            if let Err(error) = verify_installed_windows_runtime(user_root, &state.runtime_release)
            {
                health_issues.push(format!("Windows Runtime is unhealthy: {error}"));
            }
        }
        match registration {
            Ok(Some(evidence)) if registration_matches_install_state(&evidence, &state) => {
                if !enabled {
                    health_issues.push(
                        "Windows debugger registration remains recorded and requires recovery."
                            .to_string(),
                    );
                }
            }
            Ok(Some(_)) => health_issues.push(
                "Windows debugger registration does not match durable install state.".to_string(),
            ),
            Ok(None) if enabled => {
                health_issues.push("Windows debugger registration evidence is missing.".to_string())
            }
            Ok(None) => {}
            Err(error) => health_issues.push(format!(
                "Windows debugger registration evidence is unhealthy: {error}"
            )),
        }
        Ok(Self {
            installed: enabled && health_issues.is_empty(),
            phase: Some(state.phase),
            desired_enabled: state.desired_enabled(),
            package_full_name: Some(state.package_full_name),
            runtime_release: Some(state.runtime_release),
            health_issues,
        })
    }

    fn without_install_state(
        registration: Result<Option<WindowsDebugRegistrationEvidence>, String>,
        state_error: Option<String>,
    ) -> Self {
        let mut health_issues = Vec::new();
        if let Some(error) = state_error {
            health_issues.push(format!("Windows install state is unhealthy: {error}"));
        }

        let package_full_name = match registration {
            Ok(Some(evidence)) if evidence.kind == WindowsDebugRegistrationKind::Installed => {
                health_issues.push(
                    "Windows debugger registration requires recovery because primary install state is missing or unhealthy."
                        .to_string(),
                );
                Some(evidence.package_full_name)
            }
            Ok(_) => None,
            Err(error) => {
                health_issues.push(format!(
                    "Windows debugger registration evidence is unhealthy: {error}"
                ));
                None
            }
        };

        Self {
            installed: false,
            phase: None,
            desired_enabled: false,
            package_full_name,
            runtime_release: None,
            health_issues,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WindowsPackageStatus {
    pub(crate) available: bool,
    pub(crate) package_full_name: Option<String>,
    pub(crate) app_user_model_id: Option<String>,
    pub(crate) install_location: Option<PathBuf>,
    pub(crate) executable: Option<PathBuf>,
    pub(crate) architecture: Option<String>,
    pub(crate) reason: Option<String>,
}

impl WindowsPackageStatus {
    pub(crate) fn inspect() -> Self {
        match discover_codex_package() {
            Ok(app) => Self::available(app),
            Err(reason) => Self::unavailable(reason),
        }
    }

    fn available(app: WindowsCodexApp) -> Self {
        Self {
            available: true,
            package_full_name: Some(app.package_full_name),
            app_user_model_id: Some(app.app_user_model_id),
            install_location: Some(app.install_location),
            executable: Some(app.executable),
            architecture: Some(app.architecture),
            reason: None,
        }
    }

    fn unavailable(reason: String) -> Self {
        Self {
            available: false,
            package_full_name: None,
            app_user_model_id: None,
            install_location: None,
            executable: None,
            architecture: None,
            reason: Some(reason),
        }
    }
}

pub fn run_status(parsed: &ParsedCli) -> Result<(), CliFailure> {
    if parsed.app.is_some() {
        return Err(CliFailure::new(
            "--app is not supported by Windows status; the current user's official Store package is discovered automatically",
        ));
    }

    let mut spinner = (!parsed.json).then(|| Spinner::start(STATUS_PROGRESS_MESSAGE));
    let profile = crate::windows_profile::windows_user_profile().map_err(CliFailure::from)?;
    let package = WindowsPackageStatus::inspect();
    let integration = WindowsIntegrationStatus::inspect(
        &profile.join(".incodex"),
        package.package_full_name.as_deref(),
    )
    .map_err(CliFailure::from)?;
    if let Some(spinner) = &mut spinner {
        spinner.stop();
    }
    if parsed.json {
        let report = WindowsStatus {
            platform: "windows",
            package,
            integration,
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("Windows status is serializable")
        );
    } else {
        crate::terminal_presentation::print_terminal_report(&format_status(&package, &integration));
    }
    Ok(())
}

pub(crate) fn format_status(
    package: &WindowsPackageStatus,
    integration: &WindowsIntegrationStatus,
) -> String {
    format!(
        "{}\n\n{}",
        format_package_status(package, "Status"),
        format_integration_status(integration)
    )
}

pub(crate) fn format_integration_status(report: &WindowsIntegrationStatus) -> String {
    let mut lines = vec![
        incodex_core::format_step("Integration", None),
        incodex_core::format_kv(
            "Installed",
            if report.installed { "yes" } else { "no" },
            None,
        ),
    ];
    if let Some(phase) = report.phase {
        lines.push(incodex_core::format_kv(
            "Phase",
            &format!("{phase:?}"),
            None,
        ));
    }
    if let Some(release) = &report.runtime_release {
        lines.push(incodex_core::format_kv("Runtime", release, None));
    }
    for issue in &report.health_issues {
        lines.push(incodex_core::format_warn(issue, None));
    }
    lines.join("\n")
}

pub(crate) fn format_package_status(report: &WindowsPackageStatus, heading: &str) -> String {
    let mut lines = vec![
        incodex_core::format_step(heading, None),
        incodex_core::format_kv(
            "Available",
            if report.available { "yes" } else { "no" },
            None,
        ),
    ];

    if report.available {
        lines.extend([
            incodex_core::format_kv(
                "Package",
                report.package_full_name.as_deref().unwrap_or("unknown"),
                None,
            ),
            incodex_core::format_kv(
                "App ID",
                report.app_user_model_id.as_deref().unwrap_or("unknown"),
                None,
            ),
            incodex_core::format_kv(
                "Location",
                &report
                    .install_location
                    .as_deref()
                    .map_or_else(|| "unknown".to_string(), windows_path_for_display),
                None,
            ),
            incodex_core::format_kv(
                "Executable",
                &report
                    .executable
                    .as_deref()
                    .map_or_else(|| "unknown".to_string(), windows_path_for_display),
                None,
            ),
            incodex_core::format_kv(
                "Architecture",
                report.architecture.as_deref().unwrap_or("unknown"),
                None,
            ),
            incodex_core::format_ok("Official Store package is ready for `incodex open`.", None),
        ]);
    } else if let Some(reason) = &report.reason {
        lines.push(incodex_core::format_warn(reason, None));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows_helper::{publish_windows_helper, publish_windows_transient_helper};
    use crate::windows_install_state::{
        stage_windows_install_state, transition_windows_install_state, WindowsInstallState,
    };
    use crate::windows_registration::{
        stage_installed_windows_debug_registration, stage_transient_windows_debug_registration,
    };
    use crate::windows_runtime::publish_windows_runtime;
    use std::sync::atomic::{AtomicU64, Ordering};

    static STATUS_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn unavailable_status_keeps_the_reason_and_no_package_identity() {
        let report = WindowsPackageStatus::unavailable("not installed".to_string());

        assert!(!report.available);
        assert_eq!(report.reason.as_deref(), Some("not installed"));
        assert!(report.package_full_name.is_none());
        let integration = WindowsIntegrationStatus {
            installed: false,
            phase: None,
            desired_enabled: false,
            package_full_name: None,
            runtime_release: None,
            health_issues: Vec::new(),
        };
        let text = format_status(&report, &integration);
        assert!(text.starts_with("➤ Status"), "{text}");
        assert!(!text.contains("Windows Codex"), "{text}");
        assert!(text.contains("Available    no"), "{text}");
        assert!(text.contains("not installed"), "{text}");
    }

    #[test]
    fn stale_store_package_generation_is_not_reported_as_installed() {
        let sequence = STATUS_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let user_root = std::env::temp_dir().join(format!(
            "incodex-windows-status-{}-{sequence}",
            std::process::id()
        ));
        let helper = std::env::current_exe().expect("test helper path");
        let old_package = "OpenAI.Codex_1.2.3.4_x64__publisher";
        let current_package = "OpenAI.Codex_1.2.3.5_x64__publisher";
        let staged = stage_windows_install_state(
            &user_root,
            old_package,
            &helper,
            "0.5.0-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .expect("stage stale install state");
        let pending = transition_windows_install_state(
            &user_root,
            staged.epoch,
            WindowsInstallPhase::EnablePending,
        )
        .expect("record enable intent");
        transition_windows_install_state(
            &user_root,
            pending.epoch,
            WindowsInstallPhase::EnabledUnobserved,
        )
        .expect("record old enabled generation");

        let report = WindowsIntegrationStatus::inspect(&user_root, Some(current_package))
            .expect("inspect stale generation");
        assert!(!report.installed);
        assert_eq!(report.package_full_name.as_deref(), Some(old_package));
        std::fs::remove_dir_all(user_root).expect("remove stale status fixture");
    }

    fn enabled_install_fixture(
        user_root: &std::path::Path,
        with_registration: bool,
    ) -> (WindowsInstallState, std::path::PathBuf) {
        let runtime = publish_windows_runtime(user_root).expect("publish fixture Runtime");
        let runtime_release = runtime
            .release_dir
            .file_name()
            .and_then(|name| name.to_str())
            .expect("fixture Runtime release name");
        let helper_source = std::env::current_exe().expect("test helper source");
        let helper =
            publish_windows_helper(user_root, &helper_source).expect("publish fixture helper");
        let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
        let staged =
            stage_windows_install_state(user_root, package, &helper.executable, runtime_release)
                .expect("stage fixture install state");
        let pending = transition_windows_install_state(
            user_root,
            staged.epoch,
            WindowsInstallPhase::EnablePending,
        )
        .expect("record fixture enable intent");
        if with_registration {
            stage_installed_windows_debug_registration(user_root, &pending)
                .expect("stage fixture registration evidence");
        }
        let enabled = transition_windows_install_state(
            user_root,
            pending.epoch,
            WindowsInstallPhase::EnabledUnobserved,
        )
        .expect("record fixture enabled state");
        (enabled, runtime.release_dir)
    }

    #[test]
    fn enabled_state_without_registration_is_not_reported_as_healthy() {
        let sequence = STATUS_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let user_root = std::env::temp_dir().join(format!(
            "incodex-windows-status-registration-{}-{sequence}",
            std::process::id()
        ));
        let (state, _) = enabled_install_fixture(&user_root, false);

        let report = WindowsIntegrationStatus::inspect(&user_root, Some(&state.package_full_name))
            .expect("inspect missing registration");
        let text = format_integration_status(&report);

        assert!(
            !report.installed,
            "missing registration was reported installed"
        );
        assert!(text.to_ascii_lowercase().contains("registration"), "{text}");
        std::fs::remove_dir_all(user_root).expect("remove registration fixture");
    }

    #[test]
    fn registration_without_primary_state_requires_recovery() {
        let sequence = STATUS_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let user_root = std::env::temp_dir().join(format!(
            "incodex-windows-status-recovery-missing-{}-{sequence}",
            std::process::id()
        ));
        let (state, _) = enabled_install_fixture(&user_root, true);
        std::fs::remove_file(user_root.join("windows-install.json"))
            .expect("remove primary install state");

        let inspection =
            WindowsIntegrationStatus::inspect(&user_root, Some(state.package_full_name.as_str()));
        std::fs::remove_dir_all(&user_root).expect("remove missing-state fixture");
        let report = inspection.expect("inspect independent registration evidence");
        let text = format_integration_status(&report);

        assert!(!report.installed, "recovery state was reported installed");
        assert_eq!(
            report.package_full_name.as_deref(),
            Some(state.package_full_name.as_str())
        );
        assert!(text.to_ascii_lowercase().contains("recovery"), "{text}");
        assert!(text.to_ascii_lowercase().contains("registration"), "{text}");
    }

    #[test]
    fn transient_registration_without_primary_state_requires_recovery() {
        let sequence = STATUS_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let user_root = std::env::temp_dir().join(format!(
            "incodex-windows-status-recovery-transient-{}-{sequence}",
            std::process::id()
        ));
        let helper_source = std::env::current_exe().expect("test helper source");
        let helper = publish_windows_transient_helper(&user_root, &helper_source)
            .expect("publish fixture transient helper");
        let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
        stage_transient_windows_debug_registration(&user_root, package, &helper.executable)
            .expect("stage transient registration evidence");

        let inspection = WindowsIntegrationStatus::inspect(&user_root, Some(package));
        std::fs::remove_dir_all(&user_root).expect("remove transient recovery fixture");
        let report = inspection.expect("inspect transient registration evidence");
        let text = format_integration_status(&report);

        assert!(
            !report.installed,
            "transient recovery was reported installed"
        );
        assert_eq!(report.package_full_name.as_deref(), Some(package));
        assert!(text.to_ascii_lowercase().contains("transient"), "{text}");
        assert!(text.to_ascii_lowercase().contains("recovery"), "{text}");
    }

    #[test]
    fn recovery_phase_with_installed_registration_reports_recovery() {
        let sequence = STATUS_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let user_root = std::env::temp_dir().join(format!(
            "incodex-windows-status-recovery-registered-{}-{sequence}",
            std::process::id()
        ));
        let (enabled, _) = enabled_install_fixture(&user_root, true);
        let recovery = transition_windows_install_state(
            &user_root,
            enabled.epoch,
            WindowsInstallPhase::RecoveryRequired,
        )
        .expect("record fixture recovery state");

        let inspection = WindowsIntegrationStatus::inspect(
            &user_root,
            Some(recovery.package_full_name.as_str()),
        );
        std::fs::remove_dir_all(&user_root).expect("remove registered recovery fixture");
        let report = inspection.expect("inspect registered recovery state");
        let text = format_integration_status(&report);

        assert!(!report.installed, "recovery state was reported installed");
        assert_eq!(report.phase, Some(WindowsInstallPhase::RecoveryRequired));
        assert!(text.to_ascii_lowercase().contains("recovery"), "{text}");
        assert!(text.to_ascii_lowercase().contains("registration"), "{text}");
    }

    #[test]
    fn malformed_primary_state_is_reported_instead_of_aborting_status() {
        let sequence = STATUS_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let user_root = std::env::temp_dir().join(format!(
            "incodex-windows-status-recovery-malformed-{}-{sequence}",
            std::process::id()
        ));
        let (state, _) = enabled_install_fixture(&user_root, true);
        std::fs::write(user_root.join("windows-install.json"), b"{")
            .expect("corrupt primary install state");

        let inspection =
            WindowsIntegrationStatus::inspect(&user_root, Some(state.package_full_name.as_str()));
        std::fs::remove_dir_all(&user_root).expect("remove malformed-state fixture");
        let report = inspection.expect("status must retain recovery evidence");
        let text = format_integration_status(&report);

        assert!(!report.installed, "malformed state was reported installed");
        assert_eq!(
            report.package_full_name.as_deref(),
            Some(state.package_full_name.as_str())
        );
        assert!(text.to_ascii_lowercase().contains("unhealthy"), "{text}");
        assert!(text.to_ascii_lowercase().contains("registration"), "{text}");
    }

    #[test]
    fn tampered_runtime_is_not_reported_as_healthy() {
        let sequence = STATUS_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let user_root = std::env::temp_dir().join(format!(
            "incodex-windows-status-runtime-{}-{sequence}",
            std::process::id()
        ));
        let (state, release_dir) = enabled_install_fixture(&user_root, true);
        std::fs::write(release_dir.join("incodex-main.cjs"), b"tampered")
            .expect("tamper fixture Runtime");

        let report = WindowsIntegrationStatus::inspect(&user_root, Some(&state.package_full_name))
            .expect("inspect tampered Runtime");
        let text = format_integration_status(&report);

        assert!(!report.installed, "tampered Runtime was reported installed");
        assert!(text.to_ascii_lowercase().contains("runtime"), "{text}");
        std::fs::remove_dir_all(user_root).expect("remove Runtime fixture");
    }
}
