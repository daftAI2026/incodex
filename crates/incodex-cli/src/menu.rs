/**
 * [INPUT]: 依赖 menu_controller 的导航与 menu_view 的绘制，并读取终端按键。
 * [OUTPUT]: 提供 TTY 菜单项与按键映射；macOS 在 Open 后显示 Launch，Windows 隐藏 Launch。
 * [POS]: CLI TTY 菜单适配层；委托共享控制器选择命令，不执行产品命令。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use crate::menu_controller::{run_menu as run_shared_menu, MenuKey};
use crate::menu_view::MenuItem;
use crate::parse::CliCommand;

const ITEMS: &[MenuItem] = &[
    MenuItem {
        command: Some(CliCommand::Open),
        title: "Open",
        description: "Open an incognito window without patching",
    },
    #[cfg(target_os = "macos")]
    MenuItem {
        command: Some(CliCommand::Launch),
        title: "Launch",
        description: "Launch normal Codex without changing its installation",
    },
    MenuItem {
        command: Some(CliCommand::Install),
        title: "Install",
        description: "Patch the Codex app you are using",
    },
    MenuItem {
        command: Some(CliCommand::Uninstall),
        title: "Uninstall",
        description: "Restore the official Codex app",
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
        command: None,
        title: "Quit",
        description: "Exit this menu",
    },
];

pub fn run_menu() -> Result<Option<CliCommand>, String> {
    crate::lifecycle::spawn_update_notice_refresh();
    let update_message = crate::lifecycle::read_update_notice();
    let self_update_available = update_message
        .as_deref()
        .is_some_and(|message| message.ends_with("run inc update"));
    run_shared_menu(
        ITEMS,
        update_message.as_deref(),
        self_update_available,
        read_menu_key,
    )
}

fn read_menu_key() -> Result<MenuKey, String> {
    let key = crate::terminal::read_key()?;
    Ok(match key.as_slice() {
        [3] => MenuKey::Interrupt,
        [b'q'] | [b'Q'] | [0x1b] => MenuKey::Quit,
        [b'v'] | [b'V'] => MenuKey::Version,
        [b'u'] | [b'U'] => MenuKey::Update,
        [b'\r'] | [b'\n'] => MenuKey::Activate,
        [b'k'] | [b'K'] | [0x1b, b'[', b'A'] => MenuKey::Up,
        [b'j'] | [b'J'] | [0x1b, b'[', b'B'] => MenuKey::Down,
        [digit @ b'1'..=b'9'] => MenuKey::Digit(usize::from(*digit - b'0')),
        _ => MenuKey::Ignore,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_remains_the_default_menu_action() {
        assert_eq!(ITEMS[0].command, Some(CliCommand::Open));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_menu_digit_selects_the_existing_launch_command() {
        let mut keys = [MenuKey::Digit(2)].into_iter();
        let command = run_shared_menu(ITEMS, None, false, || {
            Ok(keys.next().expect("one menu key"))
        })
        .unwrap();

        assert_eq!(command, Some(CliCommand::Launch));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_menu_arrow_selection_selects_the_existing_launch_command() {
        let mut keys = [MenuKey::Down, MenuKey::Activate].into_iter();
        let command = run_shared_menu(ITEMS, None, false, || {
            Ok(keys.next().expect("two menu keys"))
        })
        .unwrap();

        assert_eq!(command, Some(CliCommand::Launch));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_menu_does_not_expose_the_macos_launch_command() {
        assert!(!ITEMS
            .iter()
            .any(|item| item.command == Some(CliCommand::Launch)));
    }
}
