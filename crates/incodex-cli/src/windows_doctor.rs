use serde::Serialize;

use crate::diagnosis_presentation::DOCTOR_PROGRESS_MESSAGE;
use crate::parse::ParsedCli;
use crate::spinner::Spinner;
use crate::windows_status::{
    format_integration_status, format_package_status, WindowsIntegrationStatus,
    WindowsPackageStatus,
};
use crate::CliFailure;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowsDoctor {
    platform: &'static str,
    package: WindowsPackageStatus,
    integration: WindowsIntegrationStatus,
    sessions: WindowsSessions,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowsSessions {
    active: usize,
    orphaned: usize,
    unknown: usize,
    findings: Vec<String>,
}

impl From<incodex_core::windows_session::WindowsSessionInspection> for WindowsSessions {
    fn from(report: incodex_core::windows_session::WindowsSessionInspection) -> Self {
        Self {
            active: report.active,
            orphaned: report.orphaned,
            unknown: report.unknown,
            findings: report.findings,
        }
    }
}

pub fn run_doctor(parsed: &ParsedCli) -> Result<(), CliFailure> {
    if parsed.deep {
        return Err(CliFailure::new(
            "--deep is not supported by Windows doctor; Store package identity and session ownership are already verified",
        ));
    }
    if parsed.app.is_some() {
        return Err(CliFailure::new(
            "--app is not supported by Windows doctor; the current user's official Store package is discovered automatically",
        ));
    }

    let mut spinner = (!parsed.json).then(|| Spinner::start(DOCTOR_PROGRESS_MESSAGE));
    let profile = crate::windows_profile::windows_user_profile().map_err(CliFailure::from)?;
    let report = WindowsDoctor {
        platform: "windows",
        package: WindowsPackageStatus::inspect(),
        integration: WindowsIntegrationStatus::inspect(&profile.join(".incodex"))
            .map_err(CliFailure::from)?,
        sessions: incodex_core::windows_session::inspect_windows_sessions(
            &profile.join(".incodex"),
        )
        .into(),
    };
    if let Some(spinner) = &mut spinner {
        spinner.stop();
    }
    if parsed.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("Windows doctor is serializable")
        );
    } else {
        crate::terminal_presentation::print_terminal_report(&format_doctor(&report));
    }
    Ok(())
}

fn format_doctor(report: &WindowsDoctor) -> String {
    let sessions = &report.sessions;
    let mut lines = vec![
        format_package_status(&report.package, "App"),
        String::new(),
        format_integration_status(&report.integration),
    ];
    lines.extend([
        String::new(),
        incodex_core::format_step("Sessions", None),
        incodex_core::format_kv("Active", &sessions.active.to_string(), None),
        incodex_core::format_kv("Orphaned", &sessions.orphaned.to_string(), None),
        incodex_core::format_kv("Unknown", &sessions.unknown.to_string(), None),
    ]);
    if sessions.orphaned == 0 && sessions.unknown == 0 {
        lines.push(incodex_core::format_ok(
            "No orphaned or unverifiable Windows sessions.",
            None,
        ));
    } else {
        if sessions.orphaned > 0 {
            lines.push(incodex_core::format_warn(
                "Orphaned sessions are removed before the next `incodex open`.",
                None,
            ));
        }
        for finding in &sessions.findings {
            lines.push(incodex_core::format_warn(finding, None));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_findings_are_visible_in_text_output() {
        let report = WindowsDoctor {
            platform: "windows",
            package: WindowsPackageStatus {
                available: false,
                package_full_name: None,
                app_user_model_id: None,
                install_location: None,
                executable: None,
                architecture: None,
                reason: Some("not installed".to_string()),
            },
            integration: WindowsIntegrationStatus {
                installed: false,
                phase: None,
                desired_enabled: false,
                package_full_name: None,
                runtime_release: None,
            },
            sessions: WindowsSessions {
                active: 0,
                orphaned: 1,
                unknown: 1,
                findings: vec!["owner cannot be verified".to_string()],
            },
        };

        let text = format_doctor(&report);
        assert!(text.starts_with("➤ App"), "{text}");
        assert!(!text.contains("Windows Doctor"), "{text}");
        assert!(text.contains("Orphaned     1"), "{text}");
        assert!(text.contains("Unknown      1"), "{text}");
        assert!(text.contains("owner cannot be verified"), "{text}");
    }
}
