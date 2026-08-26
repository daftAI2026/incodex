use std::io::{self, Write};

use crate::parse::CliCommand;

const MENU: &str = "\
1. Open       Open an isolated incognito Codex window
2. Status     Show official Store package availability
3. Doctor     Diagnose package health and isolated sessions
4. Version    Show CLI and Windows support information
5. Quit       Exit this menu";

pub fn run_menu() -> Result<Option<CliCommand>, String> {
    loop {
        println!("{}", incodex_core::format_step("Incodex for Windows", None));
        println!("{MENU}");
        print!("Select [1-5]: ");
        io::stdout().flush().map_err(|error| error.to_string())?;

        let mut input = String::new();
        if io::stdin()
            .read_line(&mut input)
            .map_err(|error| error.to_string())?
            == 0
        {
            return Ok(None);
        }
        let selection = input.trim();
        if matches!(selection.to_ascii_lowercase().as_str(), "5" | "q" | "quit") {
            return Ok(None);
        }
        if let Some(command) = command_for_selection(selection) {
            return Ok(Some(command));
        }
        println!(
            "{}",
            incodex_core::format_warn("Choose Open, Status, Doctor, Version, or Quit.", None)
        );
    }
}

pub fn command_for_selection(selection: &str) -> Option<CliCommand> {
    match selection.trim().to_ascii_lowercase().as_str() {
        "1" | "o" | "open" => Some(CliCommand::Open),
        "2" | "s" | "status" => Some(CliCommand::Status),
        "3" | "d" | "doctor" => Some(CliCommand::Doctor),
        "4" | "v" | "version" => Some(CliCommand::Version),
        _ => None,
    }
}
