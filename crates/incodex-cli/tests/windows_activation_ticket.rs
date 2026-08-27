#![cfg(target_os = "windows")]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::windows_activation_ticket::{
    read_windows_activation_ticket_from_command_line, WindowsActivationTicketGuard,
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    std::env::temp_dir().join(format!(
        "incodex-windows-activation-ticket-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn private_ticket_routes_only_the_matching_activation_to_its_job_and_environment() {
    let root = scratch();
    let registration = "0123456789abcdef0123456789abcdef";
    let package = "OpenAI.Codex_1.2.3.4_x64__publisher";
    let job = "Local\\Incodex-0123456789abcdef0123456789abcdef";
    let environment = BTreeMap::from([
        (
            "CODEX_HOME".to_string(),
            r"C:\Users\test\.incodex\sessions\one\codex-home".to_string(),
        ),
        ("INCODEX_INCOGNITO".to_string(), "1".to_string()),
    ]);
    let ticket = WindowsActivationTicketGuard::create(
        &root,
        registration,
        package,
        job,
        environment.clone(),
    )
    .expect("create private activation ticket");
    let argument = ticket.command_line_argument();
    let command_line = format!(r#"ChatGPT.exe "{argument}" codex://new?mode=codex"#);

    let matched = read_windows_activation_ticket_from_command_line(
        &root.join("windows-install.json"),
        registration,
        package,
        &command_line,
    )
    .expect("read matching activation ticket")
    .expect("match activation ticket");
    assert_eq!(matched.job_name, job);
    assert_eq!(matched.environment, environment);
    assert_eq!(matched.token, ticket.token());
    assert!(ticket.path().is_file());

    assert!(read_windows_activation_ticket_from_command_line(
        &root.join("windows-install.json"),
        registration,
        package,
        "ChatGPT.exe codex://new?mode=codex",
    )
    .expect("ordinary activation stays ordinary")
    .is_none());
    assert!(read_windows_activation_ticket_from_command_line(
        &root.join("windows-install.json"),
        registration,
        "Other.Package_1.2.3.4_x64__publisher",
        &command_line,
    )
    .is_err());

    let ticket_path = ticket.path().to_path_buf();
    drop(ticket);
    assert!(!ticket_path.exists());
    if root.exists() {
        fs::remove_dir_all(root).expect("remove activation ticket fixture");
    }
}
