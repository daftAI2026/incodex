pub mod help;
pub mod version;

use help::ROOT_HELP;
use version::{collect_version_facts, format_version_report};

pub fn run<I, S>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    match args.first().map(String::as_str) {
        None | Some("--help") | Some("-h") | Some("help") => {
            println!("{ROOT_HELP}");
            Ok(())
        }
        Some("--version") | Some("-V") | Some("version") => {
            print!("{}", format_version_report(&collect_version_facts()));
            Ok(())
        }
        Some(flag) if flag.starts_with('-') => {
            Err(format!("unknown flag: {flag}\n  incodex --help"))
        }
        Some(command) => Err(format!("unknown command: {command}\n  incodex --help")),
    }
}
