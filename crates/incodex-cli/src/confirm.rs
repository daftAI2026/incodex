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
