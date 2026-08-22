pub mod app_bundle;
pub(crate) mod app_quiescence;
pub mod cdp;
pub mod confirm;
pub mod diagnose;
pub(crate) mod diagnose_checks;
pub(crate) mod diagnose_format;
pub(crate) mod diagnose_signing;
pub mod help;
pub mod install;
pub mod legacy_proof;
pub mod legacy_typescript;
pub mod lifecycle;
pub mod menu;
pub mod open;
pub mod parse;
pub mod spinner;
pub mod terminal;
pub mod version;

use std::path::PathBuf;

use diagnose::{diagnose_with_root_mode, DiagnosisMode};
use diagnose_format::{diagnosis_json, format_diagnosis, format_status};
use help::{command_help, ROOT_HELP};
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
    let args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    let mut parsed = parse_cli(&args)?;
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
        CliCommand::Status | CliCommand::Doctor => {
            let target = parsed
                .app
                .as_deref()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_APP));
            let mut spinner = (!parsed.json).then(|| {
                crate::spinner::Spinner::start(if parsed.command == CliCommand::Status {
                    "Inspecting installation status"
                } else {
                    "Running diagnostics"
                })
            });
            let mode = match parsed.command {
                CliCommand::Status => DiagnosisMode::Status,
                CliCommand::Doctor if parsed.deep => DiagnosisMode::DoctorDeep,
                CliCommand::Doctor => DiagnosisMode::Doctor,
                _ => unreachable!("diagnosis branch only handles status and doctor"),
            };
            let report = diagnose_with_root_mode(&target, &incodex_core::paths::user_root(), mode);
            if let Some(spinner) = &mut spinner {
                spinner.stop();
            }
            if parsed.json {
                print!("{}", diagnosis_json(&report));
            } else if parsed.command == CliCommand::Status {
                println!("{}", format_status(&report));
            } else {
                println!("{}", format_diagnosis(&report));
            }
            Ok(())
        }
        other => Err(CliFailure::new(format!(
            "{} is not implemented in the native CLI yet",
            other.as_str()
        ))),
    }
}
