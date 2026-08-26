use std::path::PathBuf;

use serde::Serialize;

use crate::parse::ParsedCli;
use crate::windows_app::{discover_codex_package, WindowsCodexApp};
use crate::CliFailure;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowsStatus {
    platform: &'static str,
    available: bool,
    package_full_name: Option<String>,
    app_user_model_id: Option<String>,
    install_location: Option<PathBuf>,
    executable: Option<PathBuf>,
    architecture: Option<String>,
    reason: Option<String>,
}

impl WindowsStatus {
    fn inspect() -> Self {
        match discover_codex_package() {
            Ok(app) => Self::available(app),
            Err(reason) => Self::unavailable(reason),
        }
    }

    fn available(app: WindowsCodexApp) -> Self {
        Self {
            platform: "windows",
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
            platform: "windows",
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

    let report = WindowsStatus::inspect();
    if parsed.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("Windows status is serializable")
        );
    } else {
        println!("{}", format_status(&report));
    }
    Ok(())
}

fn format_status(report: &WindowsStatus) -> String {
    let mut lines = vec![
        incodex_core::format_step("Windows Codex", None),
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

    #[test]
    fn unavailable_status_keeps_the_reason_and_no_package_identity() {
        let report = WindowsStatus::unavailable("not installed".to_string());

        assert!(!report.available);
        assert_eq!(report.reason.as_deref(), Some("not installed"));
        assert!(report.package_full_name.is_none());
        let text = format_status(&report);
        assert!(text.contains("Available    no"), "{text}");
        assert!(text.contains("not installed"), "{text}");
    }
}
