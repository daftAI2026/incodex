use std::io::{self, Write};

use crate::parse::CliCommand;

pub(crate) const REPO_URL: &str = "https://github.com/daftAI2026/incodex";
pub(crate) const TAGLINE: &str = "Incognito toggle for Codex desktop.";
const ENTER_MENU_SCREEN: &str = "\u{1b}[?25l\u{1b}[H\u{1b}[J";
const LEAVE_MENU_SCREEN: &str = "\u{1b}[H\u{1b}[J\u{1b}[?25h";
pub(crate) const BANNER: &str = r#"  _____   _   _    _____    ____    _____    ______  __   __
 |_   _| | \ | |  / ____|  / __ \  |  __ \  |  ____| \ \ / /
   | |   |  \| | | |      | |  | | | |  | | | |__     \ V /
   | |   | . ` | | |      | |  | | | |  | | |  __|     > <
  _| |_  | |\  | | |____  | |__| | | |__| | | |____   / . \
 |_____| |_| \_|  \_____|  \____/  |_____/  |______| /_/ \_\"#;

pub(crate) struct MenuItem {
    pub(crate) command: Option<CliCommand>,
    pub(crate) title: &'static str,
    pub(crate) description: &'static str,
}

pub(crate) fn render_menu_lines(
    items: &[MenuItem],
    selected: Option<usize>,
    notice: Option<&str>,
    controls: &str,
) -> Vec<String> {
    let mut lines = vec![
        paint("0;32", BANNER),
        String::new(),
        paint("1;34", &format!("  {REPO_URL}")),
        paint("0;32", &format!("  {TAGLINE}")),
    ];
    if let Some(message) = notice {
        lines.push(String::new());
        lines.push(paint("0;32", &format!("  {message}")));
    }
    lines.push(String::new());
    let title_width = items.iter().map(|item| item.title.len()).max().unwrap_or(0);
    for (index, item) in items.iter().enumerate() {
        let body = format!(
            "{}. {:title_width$}  {}",
            index + 1,
            item.title,
            item.description
        );
        lines.push(if selected == Some(index) {
            paint("0;36", &format!("➤ {body}"))
        } else {
            format!("  {body}")
        });
    }
    lines.push(String::new());
    lines.push(paint("0;38;5;244", controls));
    lines
}

pub(crate) fn draw_menu_lines(lines: &[String]) -> Result<(), String> {
    print!("{}", menu_frame(lines));
    io::stdout().flush().map_err(|error| error.to_string())
}

fn menu_frame(lines: &[String]) -> String {
    let body = lines.join("\n");
    let frame = body
        .lines()
        .map(|line| format!("\r\u{1b}[2K{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{ENTER_MENU_SCREEN}{frame}\n\u{1b}[J")
}

fn paint(code: &str, text: &str) -> String {
    format!("\u{1b}[{code}m{text}\u{1b}[0m")
}

pub(crate) struct CursorGuard;

impl Drop for CursorGuard {
    fn drop(&mut self) {
        print!("{LEAVE_MENU_SCREEN}");
        let _ = io::stdout().flush();
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
            command: None,
            title: "Quit",
            description: "Exit this menu",
        },
    ];

    #[test]
    fn shared_styles_are_the_main_menu_styles() {
        let lines = render_menu_lines(ITEMS, Some(0), None, "controls");

        assert!(lines[0].starts_with("\u{1b}[0;32m  _____"));
        assert!(lines[2].starts_with("\u{1b}[1;34m  https://"));
        assert!(lines[3].starts_with("\u{1b}[0;32m  Incognito"));
        assert!(lines[5].starts_with("\u{1b}[0;36m➤ 1. Open"));
        assert!(lines.last().unwrap().starts_with("\u{1b}[0;38;5;244m"));
    }

    #[test]
    fn shared_frame_owns_the_proven_cursor_and_erase_contract() {
        let frame = menu_frame(&["first".to_string(), "second".to_string()]);

        assert_eq!(
            frame,
            "\u{1b}[?25l\u{1b}[H\u{1b}[J\r\u{1b}[2Kfirst\n\r\u{1b}[2Ksecond\n\u{1b}[J"
        );
        assert_eq!(LEAVE_MENU_SCREEN, "\u{1b}[H\u{1b}[J\u{1b}[?25h");
    }
}
