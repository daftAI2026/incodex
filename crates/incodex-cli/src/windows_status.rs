use std::path::PathBuf;

use serde::Serialize;

use crate::diagnosis_presentation::STATUS_PROGRESS_MESSAGE;
use crate::parse::ParsedCli;
use crate::spinner::Spinner;
use crate::windows_app::{discover_codex_package, WindowsCodexApp};
use crate::windows_install_state::{read_windows_install_state, WindowsInstallPhase};
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
}

impl WindowsIntegrationStatus {
    pub(crate) fn inspect(user_root: &std::path::Path) -> Result<Self, String> {
        let Some(state) = read_windows_install_state(user_root)? else {
            return Ok(Self {
                installed: false,
                phase: None,
                desired_enabled: false,
                package_full_name: None,
                runtime_release: None,
            });
        };
        Ok(Self {
            installed: matches!(
                state.phase,
                WindowsInstallPhase::EnabledUnobserved | WindowsInstallPhase::EnabledObserved
            ) && state.desired_enabled(),
            phase: Some(state.phase),
            desired_enabled: state.desired_enabled(),
            package_full_name: Some(state.package_full_name),
            runtime_release: Some(state.runtime_release),
        })
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
    let integration =
        WindowsIntegrationStatus::inspect(&profile.join(".incodex")).map_err(CliFailure::from)?;
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
                    .map_or_else(|| "unknown".to_string(), |path| path.display().to_string()),
                None,
            ),
            incodex_core::format_kv(
                "Executable",
                &report
                    .executable
                    .as_deref()
                    .map_or_else(|| "unknown".to_string(), |path| path.display().to_string()),
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
    use crate::windows_install_state::{
        stage_windows_install_state, transition_windows_install_state,
    };
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
}
