fn main() {
    if let Err(message) = incodex_cli::run(std::env::args().skip(1)) {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
