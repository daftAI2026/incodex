use crate::parse::CliCommand;

pub(crate) const REPO_URL: &str = "https://github.com/daftAI2026/incodex";
pub(crate) const TAGLINE: &str = "Incognito toggle for Codex desktop.";
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

fn paint(code: &str, text: &str) -> String {
    format!("\u{1b}[{code}m{text}\u{1b}[0m")
}
