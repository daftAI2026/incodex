use std::io::IsTerminal;

fn main() {
    if let Err(failure) = incodex_cli::run_with_exit_code(std::env::args().skip(1)) {
        if !failure.message().is_empty() {
            eprintln!(
                "{}",
                incodex_core::format_error(
                    failure.message(),
                    Some(std::io::stderr().is_terminal())
                )
            );
        }
        std::process::exit(failure.exit_code());
    }
}
