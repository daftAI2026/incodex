use crate::menu_view::{draw_menu_lines, render_menu_lines, CursorGuard, MenuItem};
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
    let mut selected = 0_usize;
    crate::lifecycle::spawn_update_notice_refresh();
    let update_message = crate::lifecycle::read_update_notice();
    let self_update_available = update_message
        .as_deref()
        .is_some_and(|message| message.ends_with("run inc update"));
    let _cursor = CursorGuard;
    loop {
        draw(selected, update_message.as_deref(), self_update_available)?;
        let key = crate::terminal::read_key()?;
        match key.as_slice() {
            [3] => return Err("interrupted".into()),
            [b'q'] | [b'Q'] | [0x1b] => return Ok(None),
            [b'v'] | [b'V'] => return Ok(Some(CliCommand::Version)),
            [b'u'] | [b'U'] if self_update_available => {
                return Ok(Some(CliCommand::Update));
            }
            [b'\r'] | [b'\n'] => return Ok(ITEMS[selected].command),
            [b'k'] | [b'K'] | [0x1b, b'[', b'A'] => {
                selected = (selected + ITEMS.len() - 1) % ITEMS.len();
            }
            [b'j'] | [b'J'] | [0x1b, b'[', b'B'] => {
                selected = (selected + 1) % ITEMS.len();
            }
            [digit @ b'1'..=b'9'] => {
                let index = usize::from(*digit - b'1');
                if let Some(item) = ITEMS.get(index) {
                    return Ok(item.command);
                }
            }
            _ => {}
        }
    }
}

fn draw(
    selected: usize,
    update_message: Option<&str>,
    self_update_available: bool,
) -> Result<(), String> {
    let controls = if self_update_available {
        format!(
            "↑↓ | Enter | U Update | V Version | Q Quit | 1-{} Jump",
            ITEMS.len()
        )
    } else {
        format!("↑↓ | Enter | V Version | Q Quit | 1-{} Jump", ITEMS.len())
    };
    let lines = render_menu_lines(ITEMS, Some(selected), update_message, &controls);
    draw_menu_lines(&lines)
}
