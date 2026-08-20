use std::io::IsTerminal;

fn main() {
    if let Err(message) = incodex_cli::run(std::env::args().skip(1)) {
        eprintln!(
            "{}",
            incodex_core::format_error(&message, Some(std::io::stderr().is_terminal()))
        );
        std::process::exit(1);
    }
}
