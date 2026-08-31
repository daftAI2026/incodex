#[cfg(not(target_os = "windows"))]
pub mod app_bundle;
#[cfg(not(target_os = "windows"))]
pub(crate) mod app_quiescence;
pub mod cdp;
pub mod confirm;
#[cfg(not(target_os = "windows"))]
pub mod diagnose;
#[cfg(not(target_os = "windows"))]
pub(crate) mod diagnose_checks;
#[cfg(not(target_os = "windows"))]
pub(crate) mod diagnose_format;
#[cfg(not(target_os = "windows"))]
mod diagnose_runtime;
#[cfg(not(target_os = "windows"))]
pub(crate) mod diagnose_signing;
mod diagnosis_presentation;
pub(crate) mod friendly_name;
pub mod help;
#[cfg(not(target_os = "windows"))]
pub mod install;
#[cfg(not(target_os = "windows"))]
mod install_keychain_advice;
#[cfg(not(target_os = "windows"))]
pub mod legacy_proof;
#[cfg(not(target_os = "windows"))]
pub mod legacy_typescript;
#[cfg(not(target_os = "windows"))]
pub mod lifecycle;
mod locale;
#[cfg(not(target_os = "windows"))]
pub mod menu;
mod menu_controller;
mod menu_view;
#[cfg(not(target_os = "windows"))]
pub mod open;
mod open_presentation;
pub mod parse;
pub mod profile_mask;
pub mod spinner;
mod stable_release;
#[cfg(not(target_os = "windows"))]
pub mod terminal;
mod terminal_presentation;
pub mod version;
#[cfg(target_os = "windows")]
pub mod windows_activation;
#[cfg(target_os = "windows")]
pub mod windows_activation_capability;
#[cfg(target_os = "windows")]
pub mod windows_app;
#[cfg(target_os = "windows")]
pub(crate) mod windows_cleanup;
#[cfg(target_os = "windows")]
pub mod windows_console;
#[cfg(target_os = "windows")]
pub mod windows_doctor;
#[cfg(target_os = "windows")]
mod windows_file;
#[cfg(target_os = "windows")]
pub mod windows_helper;
#[cfg(target_os = "windows")]
pub mod windows_install;
#[cfg(target_os = "windows")]
pub mod windows_install_state;
#[cfg(target_os = "windows")]
pub mod windows_launch;
#[cfg(target_os = "windows")]
pub(crate) mod windows_locale;
#[cfg(target_os = "windows")]
pub mod windows_menu;
#[cfg(target_os = "windows")]
pub mod windows_open;
#[cfg(target_os = "windows")]
pub mod windows_process;
#[cfg(target_os = "windows")]
pub(crate) mod windows_profile;
#[cfg(target_os = "windows")]
pub mod windows_registration;
#[cfg(target_os = "windows")]
pub mod windows_runtime;
#[cfg(target_os = "windows")]
mod windows_runtime_lifecycle;
#[cfg(target_os = "windows")]
pub mod windows_runtime_open;
#[cfg(target_os = "windows")]
pub mod windows_self_uninstall;
#[cfg(target_os = "windows")]
pub mod windows_status;
#[cfg(target_os = "windows")]
pub(crate) mod windows_system;
#[cfg(target_os = "windows")]
pub mod windows_update;
#[cfg(target_os = "windows")]
mod windows_update_flow;

#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;

#[cfg(not(target_os = "windows"))]
use diagnose::{diagnose_with_root_mode, DiagnosisMode};
#[cfg(not(target_os = "windows"))]
use diagnose_format::{diagnosis_json, format_diagnosis, format_status};
#[cfg(not(target_os = "windows"))]
use help::{command_help, ROOT_HELP};
#[cfg(target_os = "windows")]
use help::{windows_command_help as command_help, WINDOWS_ROOT_HELP as ROOT_HELP};
#[cfg(not(target_os = "windows"))]
use incodex_core::paths::DEFAULT_APP;
use parse::{parse_cli, CliCommand};
use version::{collect_version_facts, format_version_report};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliFailure {
    message: String,
    exit_code: i32,
}

impl CliFailure {
    pub fn new(message: impl Into<String>) -> Self {
        Self::with_code(1, message)
    }

    pub fn with_code(exit_code: i32, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code,
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    fn into_message(self) -> String {
        self.message
    }
}

impl From<String> for CliFailure {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<&str> for CliFailure {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

/// Backward-compatible library entry point for callers that only need text.
/// The native executable uses [`run_with_exit_code`] to preserve lifecycle
/// failure classes at the process boundary.
pub fn run<I, S>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    run_with_exit_code(args).map_err(CliFailure::into_message)
}

pub fn run_with_exit_code<I, S>(args: I) -> Result<(), CliFailure>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    #[cfg(not(target_os = "windows"))]
    if lifecycle::run_update_notice_worker() {
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    windows_console::enable_virtual_terminal();
    let args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    #[cfg(target_os = "windows")]
    if let Some(result) = windows_activation::try_run_package_debugger(&args) {
        return result.map_err(CliFailure::from);
    }
    #[cfg(target_os = "windows")]
    if let Some(result) = windows_activation::try_run_installed_package_debugger(&args) {
        return result.map_err(CliFailure::from);
    }
    #[cfg(target_os = "windows")]
    if let Some(result) = windows_runtime_open::try_run_windows_runtime_open(&args) {
        return result.map_err(CliFailure::from);
    }
    let parsed = parse_cli(&args)?;
    if parsed.command == CliCommand::Version {
        print!("{}", format_version_report(&collect_version_facts()));
        return Ok(());
    }
    if parsed.command == CliCommand::Help || parsed.help {
        let text = if parsed.command == CliCommand::Help || parsed.command == CliCommand::Menu {
            ROOT_HELP
        } else {
            command_help(parsed.command)
        };
        println!("{text}");
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let mut parsed = parsed;
        if parsed.command == CliCommand::Menu {
            if !windows_console::is_tty() {
                println!("{ROOT_HELP}");
                return Ok(());
            }
            let Some(command) = windows_menu::run_menu().map_err(CliFailure::from)? else {
                return Ok(());
            };
            parsed.command = command;
            if command == CliCommand::Version {
                print!("{}", format_version_report(&collect_version_facts()));
                return Ok(());
            }
        }
        if parsed.command == CliCommand::Open {
            return crate::windows_open::run_open(&parsed);
        }
        if parsed.command == CliCommand::Status {
            return crate::windows_status::run_status(&parsed);
        }
        if parsed.command == CliCommand::Doctor {
            return crate::windows_doctor::run_doctor(&parsed);
        }
        if parsed.command == CliCommand::Install {
            return crate::windows_install::run_install(&parsed).map_err(CliFailure::from);
        }
        if parsed.command == CliCommand::Uninstall {
            return crate::windows_install::run_uninstall(&parsed).map_err(CliFailure::from);
        }
        if parsed.command == CliCommand::Runtime {
            return crate::windows_update::run_runtime(&parsed).map_err(CliFailure::from);
        }
        if parsed.command == CliCommand::Update {
            return crate::windows_update::run_update(&parsed).map_err(CliFailure::from);
        }
        if parsed.command == CliCommand::SelfUninstall {
            return crate::windows_self_uninstall::run_self_uninstall(&parsed)
                .map_err(CliFailure::from);
        }
        Err(CliFailure::new(format!(
            "{} is not supported on Windows yet",
            parsed.command.as_str()
        )))
    }

    #[cfg(not(target_os = "windows"))]
    let mut parsed = parsed;

    #[cfg(not(target_os = "windows"))]
    if parsed.command == CliCommand::Menu {
        if !terminal::is_tty() {
            println!("{ROOT_HELP}");
            return Ok(());
        }
        let Some(command) = menu::run_menu()? else {
            return Ok(());
        };
        parsed.command = command;
        parsed.live = matches!(command, CliCommand::Install | CliCommand::Uninstall);
        if command == CliCommand::Version {
            print!("{}", format_version_report(&collect_version_facts()));
            return Ok(());
        }
    }

    #[cfg(not(target_os = "windows"))]
    match parsed.command {
        CliCommand::Open => crate::open::run_open(&parsed),
        CliCommand::Runtime => crate::lifecycle::run_runtime(&parsed).map_err(CliFailure::from),
        CliCommand::Update => crate::lifecycle::run_update(&parsed).map_err(CliFailure::from),
        CliCommand::SelfUninstall => {
            crate::lifecycle::run_self_uninstall(&parsed).map_err(CliFailure::from)
        }
        CliCommand::Install => crate::install::run_install(&parsed).map_err(CliFailure::from),
        CliCommand::Uninstall => crate::install::run_uninstall(&parsed).map_err(CliFailure::from),
        CliCommand::Recover => crate::install::run_recover(&parsed).map_err(CliFailure::from),
        CliCommand::Status | CliCommand::Doctor => run_diagnosis(&parsed),
        other => Err(CliFailure::new(format!(
            "{} is not implemented in the native CLI yet",
            other.as_str()
        ))),
    }
}

#[cfg(not(target_os = "windows"))]
fn run_diagnosis(parsed: &parse::ParsedCli) -> Result<(), CliFailure> {
    let target = parsed
        .app
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_APP));
    let mut spinner = (!parsed.json).then(|| {
        crate::spinner::Spinner::start(if parsed.command == CliCommand::Status {
            crate::diagnosis_presentation::STATUS_PROGRESS_MESSAGE
        } else {
            crate::diagnosis_presentation::DOCTOR_PROGRESS_MESSAGE
        })
    });
    let mode = match parsed.command {
        CliCommand::Status => DiagnosisMode::Status,
        CliCommand::Doctor if parsed.deep => DiagnosisMode::DoctorDeep,
        CliCommand::Doctor => DiagnosisMode::Doctor,
        _ => unreachable!("diagnosis branch only handles status and doctor"),
    };
    let root = incodex_core::paths::user_root();
    let report = diagnose_with_root_mode(&target, &root, mode);
    if let Some(spinner) = &mut spinner {
        spinner.stop();
    }
    if parsed.json {
        print!("{}", diagnosis_json(&report, &root));
    } else if parsed.command == CliCommand::Status {
        crate::terminal_presentation::print_terminal_report(&format_status(&report));
    } else {
        crate::terminal_presentation::print_terminal_report(&format_diagnosis(&report, &root));
    }
    Ok(())
}
