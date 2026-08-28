#![cfg(target_os = "windows")]

use incodex_cli::windows_activation_capability::{
    activation_capability_from_command_line, WindowsActivationCapability,
};

#[test]
fn isolated_profile_routes_only_an_explicit_activation_to_private_pipes() {
    let user_data_dir = r"C:\Users\林 纳斯\.incodex\sessions\s-one\chromium";
    let capability = WindowsActivationCapability::from_user_data_dir(user_data_dir)
        .expect("derive activation capability");
    let argument = format!("--user-data-dir={user_data_dir}");
    let command_line = format!(r#"ChatGPT.exe "{argument}" codex://new?mode=codex"#);

    assert_eq!(
        activation_capability_from_command_line(&command_line)
            .expect("parse activation command line"),
        Some(capability.clone())
    );
    assert_eq!(
        activation_capability_from_command_line("ChatGPT.exe codex://new?mode=codex")
            .expect("ordinary activation stays ordinary"),
        None
    );
    assert_eq!(
        activation_capability_from_command_line(
            r"ChatGPT.exe --user-data-dir=C:\Users\Linus\ordinary-profile"
        )
        .expect("ordinary custom profile stays ordinary"),
        None
    );
    assert!(
        activation_capability_from_command_line(&format!(r#"{command_line} "{argument}""#))
            .is_err()
    );
    assert!(capability.job_name().starts_with(r"Local\Incodex-"));
    assert!(capability
        .environment_pipe_name()
        .starts_with(r"\\.\pipe\Incodex-Activation-Environment-"));
    assert!(capability.job_name().ends_with(capability.token()));
    assert!(capability
        .environment_pipe_name()
        .ends_with(capability.token()));
}
