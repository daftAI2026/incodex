use crate::menu_controller::run_menu as run_shared_menu;
use crate::menu_view::MenuItem;
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

pub fn run_menu() -> Result<Option<CliCommand>, String> {
    run_shared_menu(ITEMS, None, false, crate::windows_console::read_menu_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_adapter_only_declares_approved_commands() {
        let commands = ITEMS
            .iter()
            .filter_map(|item| item.command)
            .collect::<Vec<_>>();

        assert_eq!(
            commands,
            [
                CliCommand::Open,
                CliCommand::Status,
                CliCommand::Doctor,
            ]
        );
    }
}
