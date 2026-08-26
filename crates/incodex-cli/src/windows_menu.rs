use crate::menu_view::{draw_menu_lines, render_menu_lines, CursorGuard, MenuItem};
use crate::parse::CliCommand;
use crate::windows_console::{read_menu_key, MenuKey};

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

fn menu_lines(selected: usize) -> Vec<String> {
    render_menu_lines(
        ITEMS,
        Some(selected),
        None,
        &format!("↑↓ | Enter | V Version | Q Quit | 1-{} Jump", ITEMS.len()),
    )
}

pub fn run_menu() -> Result<Option<CliCommand>, String> {
    let mut selected = 0usize;
    let _cursor = CursorGuard;
    loop {
        draw_menu_lines(&menu_lines(selected))?;
        let key = read_menu_key()?;
        if key == MenuKey::Interrupt {
            return Err("interrupted".to_string());
        }
        match apply_menu_key(selected, key) {
            MenuDecision::Select(next) => selected = next,
            MenuDecision::Run(command) => return Ok(command),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuDecision {
    Select(usize),
    Run(Option<CliCommand>),
}

fn apply_menu_key(selected: usize, key: MenuKey) -> MenuDecision {
    match key {
        MenuKey::Up => MenuDecision::Select((selected + ITEMS.len() - 1) % ITEMS.len()),
        MenuKey::Down => MenuDecision::Select((selected + 1) % ITEMS.len()),
        MenuKey::Activate => MenuDecision::Run(ITEMS[selected].command),
        MenuKey::Quit => MenuDecision::Run(None),
        MenuKey::Version => MenuDecision::Run(Some(CliCommand::Version)),
        MenuKey::Digit(number) => number
            .checked_sub(1)
            .and_then(|index| ITEMS.get(index))
            .map_or(MenuDecision::Select(selected), |item| {
                MenuDecision::Run(item.command)
            }),
        MenuKey::Interrupt | MenuKey::Ignore => MenuDecision::Select(selected),
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
    use super::*;

    #[test]
    fn windows_menu_preserves_the_product_branding() {
        let menu = menu_lines(0).join("\n");
        assert!(menu.contains("_____   _   _"));
        assert!(menu.contains("https://github.com/daftAI2026/incodex"));
        assert!(menu.contains("Incognito toggle for Codex desktop."));
    }

    #[test]
    fn windows_menu_uses_the_same_immediate_navigation_contract() {
        assert_eq!(apply_menu_key(0, MenuKey::Down), MenuDecision::Select(1));
        assert_eq!(apply_menu_key(0, MenuKey::Up), MenuDecision::Select(4));
        assert_eq!(
            apply_menu_key(0, MenuKey::Activate),
            MenuDecision::Run(Some(CliCommand::Open))
        );
        assert_eq!(
            apply_menu_key(0, MenuKey::Digit(4)),
            MenuDecision::Run(Some(CliCommand::Version))
        );
        assert_eq!(apply_menu_key(0, MenuKey::Quit), MenuDecision::Run(None));
    }
}
