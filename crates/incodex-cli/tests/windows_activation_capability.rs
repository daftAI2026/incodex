#![cfg(target_os = "windows")]

use incodex_cli::windows_activation_capability::{
    activation_token_from_command_line, WindowsActivationCapability,
};

#[test]
fn random_capability_routes_only_an_explicit_activation_to_private_pipes() {
    let capability = WindowsActivationCapability::create().expect("create activation capability");
    let argument = capability.command_line_argument();
    let command_line = format!(r#"ChatGPT.exe "{argument}" codex://new?mode=codex"#);

    assert_eq!(
        activation_token_from_command_line(&command_line).expect("parse activation command line"),
        Some(capability.token().to_string())
    );
    assert_eq!(
        activation_token_from_command_line("ChatGPT.exe codex://new?mode=codex")
            .expect("ordinary activation stays ordinary"),
        None
    );
    assert!(activation_token_from_command_line(&format!("{command_line} {argument}")).is_err());
    assert!(capability
        .job_name()
        .starts_with(r"Local\Incodex-"));
    assert!(capability
        .environment_pipe_name()
        .starts_with(r"\\.\pipe\Incodex-Activation-Environment-"));
    assert!(capability.job_name().ends_with(capability.token()));
    assert!(capability
        .environment_pipe_name()
        .ends_with(capability.token()));
}
