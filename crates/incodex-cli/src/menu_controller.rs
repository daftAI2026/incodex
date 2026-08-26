use crate::menu_view::{draw_menu_lines, render_menu_lines, CursorGuard, MenuItem};
use crate::parse::CliCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuKey {
    Up,
    Down,
    Activate,
    Quit,
    Update,
    Version,
    Digit(usize),
    Interrupt,
    Ignore,
}

pub(crate) fn run_menu(
    items: &[MenuItem],
    notice: Option<&str>,
    self_update_available: bool,
    mut read_key: impl FnMut() -> Result<MenuKey, String>,
) -> Result<Option<CliCommand>, String> {
    let mut selected = 0_usize;
    let _cursor = CursorGuard;
    loop {
        draw_menu_lines(&render_menu_lines(
            items,
            Some(selected),
            notice,
            &controls(items.len(), self_update_available),
        ))?;
        match apply_menu_key(items, selected, read_key()?, self_update_available) {
            MenuDecision::Select(next) => selected = next,
            MenuDecision::Run(command) => return Ok(command),
            MenuDecision::Interrupt => return Err("interrupted".to_string()),
        }
    }
}

fn controls(item_count: usize, self_update_available: bool) -> String {
    if self_update_available {
        format!("↑↓ | Enter | U Update | V Version | Q Quit | 1-{item_count} Jump")
    } else {
        format!("↑↓ | Enter | V Version | Q Quit | 1-{item_count} Jump")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuDecision {
    Select(usize),
    Run(Option<CliCommand>),
    Interrupt,
}

fn apply_menu_key(
    items: &[MenuItem],
    selected: usize,
    key: MenuKey,
    self_update_available: bool,
) -> MenuDecision {
    match key {
        MenuKey::Up => MenuDecision::Select((selected + items.len() - 1) % items.len()),
        MenuKey::Down => MenuDecision::Select((selected + 1) % items.len()),
        MenuKey::Activate => MenuDecision::Run(items[selected].command),
        MenuKey::Quit => MenuDecision::Run(None),
        MenuKey::Update if self_update_available => MenuDecision::Run(Some(CliCommand::Update)),
        MenuKey::Version => MenuDecision::Run(Some(CliCommand::Version)),
        MenuKey::Digit(number) => number
            .checked_sub(1)
            .and_then(|index| items.get(index))
            .map_or(MenuDecision::Select(selected), |item| {
                MenuDecision::Run(item.command)
            }),
        MenuKey::Interrupt => MenuDecision::Interrupt,
        MenuKey::Update | MenuKey::Ignore => MenuDecision::Select(selected),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            command: None,
            title: "Quit",
            description: "Exit this menu",
        },
    ];

    #[test]
    fn all_platforms_share_the_main_menu_navigation_contract() {
        assert_eq!(
            apply_menu_key(ITEMS, 0, MenuKey::Down, false),
            MenuDecision::Select(1)
        );
        assert_eq!(
            apply_menu_key(ITEMS, 0, MenuKey::Up, false),
            MenuDecision::Select(2)
        );
        assert_eq!(
            apply_menu_key(ITEMS, 0, MenuKey::Activate, false),
            MenuDecision::Run(Some(CliCommand::Open))
        );
        assert_eq!(
            apply_menu_key(ITEMS, 0, MenuKey::Digit(2), false),
            MenuDecision::Run(Some(CliCommand::Status))
        );
        assert_eq!(
            apply_menu_key(ITEMS, 0, MenuKey::Quit, false),
            MenuDecision::Run(None)
        );
    }

    #[test]
    fn update_shortcut_only_runs_when_the_shared_notice_allows_it() {
        assert_eq!(
            apply_menu_key(ITEMS, 1, MenuKey::Update, false),
            MenuDecision::Select(1)
        );
        assert_eq!(
            apply_menu_key(ITEMS, 1, MenuKey::Update, true),
            MenuDecision::Run(Some(CliCommand::Update))
        );
    }
}
