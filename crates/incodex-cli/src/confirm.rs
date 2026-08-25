use std::io::{self, Write};

pub const CONFIRM_PROMPT: &str = "Press Enter to confirm, ESC to cancel: ";

pub fn ask_to_continue() -> Result<bool, String> {
    print!("{CONFIRM_PROMPT}");
    io::stdout().flush().map_err(|err| err.to_string())?;
    let key = crate::terminal::read_key()?;
    println!();
    match key.as_slice() {
        [3] => Err("interrupted".into()),
        [b'\r'] | [b'\n'] => Ok(true),
        _ => Ok(false),
    }
}

pub(crate) fn ensure_confirmed(command: &str, skip: bool) -> Result<(), String> {
    if skip {
        return Ok(());
    }
    if crate::terminal::is_tty() {
        return if ask_to_continue()? {
            Ok(())
        } else {
            Err("aborted".into())
        };
    }
    Err(format!(
        "non-interactive {command} requires --yes\n  incodex {command} --yes"
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn skipped_confirmation_returns_before_terminal_detection() {
        assert_eq!(super::ensure_confirmed("install", true), Ok(()));
    }
}
