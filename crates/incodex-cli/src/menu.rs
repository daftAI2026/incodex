use crate::menu_controller::{run_menu as run_shared_menu, MenuKey};
use crate::menu_view::MenuItem;
use crate::parse::CliCommand;

const ITEMS: &[MenuItem] = &[
    MenuItem {
        command: Some(CliCommand::Open),
        title: "Open",
        description: "Open an incognito window without patching",
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
