pub mod cdp;
pub mod confirm;
pub mod diagnose;
pub mod help;
pub mod install;
pub mod lifecycle;
pub mod menu;
pub mod open;
pub mod parse;
pub mod spinner;
pub mod terminal;
pub mod version;

use std::path::PathBuf;

use diagnose::{diagnose, diagnosis_json, format_diagnosis, format_status};
use help::{command_help, ROOT_HELP};
use incodex_core::paths::DEFAULT_APP;
use parse::{parse_cli, CliCommand};
use version::{collect_version_facts, format_version_report};

pub fn run<I, S>(args: I) -> Result<(), String>
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
        CliCommand::Runtime => crate::lifecycle::run_runtime(&parsed),
        CliCommand::Update => crate::lifecycle::run_update(&parsed),
        CliCommand::SelfUninstall => crate::lifecycle::run_self_uninstall(&parsed),
        CliCommand::Install => crate::install::run_install(&parsed),
        CliCommand::Uninstall => crate::install::run_uninstall(&parsed),
        CliCommand::Recover => crate::install::run_recover(&parsed),
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
            let report = diagnose(&target);
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
        other => Err(format!(
            "{} is not implemented in the native CLI yet",
            other.as_str()
        )),
    }
}
