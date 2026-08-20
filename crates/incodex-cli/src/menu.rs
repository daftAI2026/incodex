use std::io::{self, Write};

use crate::parse::CliCommand;

const REPO_URL: &str = "https://github.com/daftAI2026/incodex";
const TAGLINE: &str = "Incognito toggle for Codex desktop.";
const BANNER: &str = r#"  _____   _   _    _____    ____    _____    ______  __   __
 |_   _| | \ | |  / ____|  / __ \  |  __ \  |  ____| \ \ / /
   | |   |  \| | | |      | |  | | | |  | | | |__     \ V /
   | |   | . ` | | |      | |  | | | |  | | |  __|     > <
  _| |_  | |\  | | |____  | |__| | | |__| | | |____   / . \
 |_____| |_| \_|  \_____|  \____/  |_____/  |______| /_/ \_\"#;

struct Item {
    command: Option<CliCommand>,
    title: &'static str,
    description: &'static str,
}

const ITEMS: &[Item] = &[
    Item {
        command: Some(CliCommand::Install),
        title: "Install",
        description: "Patch the Codex app you are using",
    },
    Item {
        command: Some(CliCommand::Uninstall),
        title: "Uninstall",
        description: "Restore the official Codex app",
    },
    Item {
        command: Some(CliCommand::Open),
        title: "Open",
        description: "Open an incognito window without patching",
    },
    Item {
        command: Some(CliCommand::Status),
        title: "Status",
        description: "Show whether Incodex is installed",
    },
    Item {
        command: Some(CliCommand::Doctor),
        title: "Doctor",
        description: "Diagnose the install and leftover sessions",
    },
    Item {
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
        .is_some_and(|message| message.ends_with("run incodex update"));
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
    let mut lines = vec![
        paint("0;32", BANNER),
        String::new(),
        paint("1;34", &format!("  {REPO_URL}")),
        paint("0;32", &format!("  {TAGLINE}")),
    ];
    if let Some(message) = update_message {
        lines.push(String::new());
        lines.push(paint("0;32", &format!("  {message}")));
    }
    lines.push(String::new());
    let title_width = ITEMS.iter().map(|item| item.title.len()).max().unwrap_or(0);
    for (index, item) in ITEMS.iter().enumerate() {
        let body = format!(
            "{}. {:title_width$}  {}",
            index + 1,
            item.title,
            item.description
        );
        lines.push(if index == selected {
            paint("0;36", &format!("➤ {body}"))
        } else {
            format!("  {body}")
        });
    }
    lines.push(String::new());
    let controls = if self_update_available {
        format!(
            "↑↓ | Enter | U Update | V Version | Q Quit | 1-{} Jump",
            ITEMS.len()
        )
    } else {
        format!("↑↓ | Enter | V Version | Q Quit | 1-{} Jump", ITEMS.len())
    };
    lines.push(paint("0;38;5;244", &controls));
    print!("\u{1b}[?25l\u{1b}[H{}\n\u{1b}[J", lines.join("\n"));
    io::stdout().flush().map_err(|err| err.to_string())
}

fn paint(code: &str, text: &str) -> String {
    format!("\u{1b}[{code}m{text}\u{1b}[0m")
}

struct CursorGuard;

impl Drop for CursorGuard {
    fn drop(&mut self) {
        print!("\u{1b}[H\u{1b}[J\u{1b}[?25h");
        let _ = io::stdout().flush();
    }
}
