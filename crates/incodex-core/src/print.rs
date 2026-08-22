use std::io::IsTerminal;

const LABEL_WIDTH: usize = 12;

fn use_color(explicit: Option<bool>) -> bool {
    explicit.unwrap_or_else(|| std::io::stdout().is_terminal())
}

fn paint(enabled: bool, code: &str, text: &str) -> String {
    if enabled {
        format!("\u{001b}[{code}m{text}\u{001b}[0m")
    } else {
        text.to_string()
    }
}

pub fn format_kv(label: &str, value: &str, color: Option<bool>) -> String {
    let pad = format!("{label:<LABEL_WIDTH$}");
    format!("  {} {value}", paint(use_color(color), "0;38;5;244", &pad))
}

pub fn format_step(message: &str, color: Option<bool>) -> String {
    format!("{} {message}", paint(use_color(color), "1;35", "➤"))
}

pub fn format_ok(message: &str, color: Option<bool>) -> String {
    format!("  {} {message}", paint(use_color(color), "0;32", "✓"))
}

pub fn format_warn(message: &str, color: Option<bool>) -> String {
    format!("  {} {message}", paint(use_color(color), "0;33", "!"))
}

pub fn format_error(message: &str, color: Option<bool>) -> String {
    format!("  {} {message}", paint(use_color(color), "0;31", "✗"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_and_marks_without_color() {
        assert!(format_kv("App", "/Applications/ChatGPT.app", Some(false)).starts_with("  "));
        assert_eq!(format_step("Status", Some(false)), "➤ Status");
        assert_eq!(
            format_warn("Codex app not found: /tmp/x", Some(false)),
            "  ! Codex app not found: /tmp/x"
        );
        assert_eq!(
            format_error("operation failed", Some(false)),
            "  ✗ operation failed"
        );
    }

    #[test]
    fn step_styles_the_complete_line_when_color_is_enabled() {
        assert_eq!(
            format_step("Install", Some(true)),
            "\u{001b}[1;35m➤ Install\u{001b}[0m"
        );
    }
}
