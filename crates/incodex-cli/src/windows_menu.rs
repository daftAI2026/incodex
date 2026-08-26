use std::io::{self, Write};

use crate::menu_view::{render_menu_lines, MenuItem};
use crate::parse::CliCommand;

const ITEMS: &[MenuItem] = &[
    MenuItem {
        command: Some(CliCommand::Open),
        title: "Open",
        description: "Open an incognito window without patching",
    },
    MenuItem {
        command: Some(CliCommand::Status),
        title: "Status",
        description: "Show whether Incodex is installed",
    },
    MenuItem {
        command: Some(CliCommand::Doctor),
        title: "Doctor",
        description: "Diagnose the install and leftover sessions",
    },
    MenuItem {
        command: Some(CliCommand::Version),
        title: "Version",
        description: "Show CLI and Windows support information",
    },
    MenuItem {
        command: None,
        title: "Quit",
        description: "Exit this menu",
    },
];

fn menu_text() -> String {
    render_menu_lines(ITEMS, None, None, "Enter a number or name | Q Quit").join("\n")
}

pub fn run_menu() -> Result<Option<CliCommand>, String> {
    loop {
        println!("{}", menu_text());
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
    let selection = selection.trim().to_ascii_lowercase();
    if let Ok(number) = selection.parse::<usize>() {
        return number
            .checked_sub(1)
            .and_then(|index| ITEMS.get(index))
            .and_then(|item| item.command);
    }
    ITEMS
        .iter()
        .find(|item| {
            let title = item.title.to_ascii_lowercase();
            selection == title || (selection.len() == 1 && title.starts_with(&selection))
        })
        .and_then(|item| item.command)
}

#[cfg(test)]
mod tests {
    use super::menu_text;

    #[test]
    fn windows_menu_preserves_the_product_branding() {
        let menu = menu_text();
        assert!(menu.contains("_____   _   _"));
        assert!(menu.contains("https://github.com/daftAI2026/incodex"));
        assert!(menu.contains("Incognito toggle for Codex desktop."));
    }
}
