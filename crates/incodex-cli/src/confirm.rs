use std::io::{self, Write};

pub const CONFIRM_PROMPT: &str = "Press Enter to confirm, ESC to cancel: ";

pub fn ask_to_continue() -> Result<bool, String> {
    print!("{CONFIRM_PROMPT}");
    io::stdout().flush().map_err(|err| err.to_string())?;
    #[cfg(not(target_os = "windows"))]
    {
        let key = crate::terminal::read_key()?;
        println!();
        return match key.as_slice() {
            [3] => Err("interrupted".into()),
            [b'\r'] | [b'\n'] => Ok(true),
            _ => Ok(false),
        };
    }
    #[cfg(target_os = "windows")]
    {
        use crate::menu_controller::MenuKey;

        let key = crate::windows_console::read_menu_key()?;
        println!();
        match key {
            MenuKey::Activate => Ok(true),
            MenuKey::Interrupt => Err("interrupted".into()),
            _ => Ok(false),
        }
    }
}

pub(crate) fn require(command: &str, yes: bool) -> Result<(), String> {
    if yes {
        return Ok(());
    }
    #[cfg(not(target_os = "windows"))]
    let interactive = crate::terminal::is_tty();
    #[cfg(target_os = "windows")]
    let interactive = crate::windows_console::is_tty();
    if interactive {
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
