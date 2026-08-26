#![cfg(target_os = "windows")]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use incodex_cli::windows_activation::{
    activate_packaged_kill_on_drop, WindowsActivationFailure, WindowsActivationRequest,
    WindowsInstalledRuntimeRegistration,
};
use incodex_cli::windows_process::WindowsProcessTree;

#[test]
fn exposes_one_packaged_activation_backend_for_the_open_lifecycle() {
    let _backend: fn(
        &WindowsActivationRequest,
    ) -> Result<WindowsProcessTree, WindowsActivationFailure> = activate_packaged_kill_on_drop;
}

#[test]
fn binds_the_persistent_debugger_to_a_stable_helper_state_and_package() {
    let package = "OpenAI.Codex_26.820.7780.0_x64__2p2nqsd0c76g0";
    let helper = Path::new(r"C:\Users\Linus Torvalds\.incodex\windows\helper.exe");
    let state = Path::new(r"C:\Users\Linus Torvalds\.incodex\windows-install.json");
    let mut environment = BTreeMap::new();
    environment.insert(
        "NODE_OPTIONS".to_string(),
        OsString::from(r"--require=C:\Users\Linus Torvalds\.incodex\runtime\bootstrap.cjs"),
    );
    let registration = WindowsInstalledRuntimeRegistration::new(
        package,
        helper,
        state,
        environment,
    )
    .expect("valid installed Runtime registration");

    assert_eq!(registration.package_full_name(), package);
    assert_eq!(
        registration.debugger_command_line(),
        r#""C:\Users\Linus Torvalds\.incodex\windows\helper.exe" __incodex_windows_installed_debugger --package OpenAI.Codex_26.820.7780.0_x64__2p2nqsd0c76g0 --state "C:\Users\Linus Torvalds\.incodex\windows-install.json""#
    );
    assert_eq!(registration.environment().last(), Some(&0));
    assert_eq!(
        registration.environment()[registration.environment().len() - 2],
        0
    );
}

#[test]
fn builds_a_store_activation_request_without_losing_windows_arguments_or_environment() {
    let mut environment = BTreeMap::new();
    environment.insert(
        "CODEX_HOME".to_string(),
        OsString::from(r"C:\Users\Linus Torvalds\.incodex\codex-home"),
    );
    environment.insert(
        "CODEX_ELECTRON_USER_DATA_PATH".to_string(),
        OsString::from(r"C:\Users\Linus Torvalds\.incodex\chromium\"),
    );
    let request = WindowsActivationRequest::new(
        "OpenAI.Codex_26.820.7780.0_x64__2p2nqsd0c76g0",
        "OpenAI.Codex_2p2nqsd0c76g0!App",
        [
            OsString::from(r"--user-data-dir=C:\Users\Linus Torvalds\.incodex\chromium\"),
            OsString::from("--remote-debugging-address=127.0.0.1"),
            OsString::from("--remote-debugging-port=49321"),
        ],
        environment,
    )
    .expect("valid activation request");

    assert_eq!(
        request.package_full_name(),
        "OpenAI.Codex_26.820.7780.0_x64__2p2nqsd0c76g0"
    );
    assert_eq!(
        request.app_user_model_id(),
        "OpenAI.Codex_2p2nqsd0c76g0!App"
    );
    assert_eq!(
        request.arguments(),
        r#""--user-data-dir=C:\Users\Linus Torvalds\.incodex\chromium\\" --remote-debugging-address=127.0.0.1 --remote-debugging-port=49321"#
    );
    assert_eq!(request.environment().last(), Some(&0));
    assert_eq!(
        request.environment()[request.environment().len() - 2],
        0,
        "environment block must end with a second NUL"
    );
}

#[test]
fn rejects_activation_identity_or_environment_with_embedded_nul() {
    assert!(WindowsActivationRequest::new(
        "OpenAI.Codex_bad\0package",
        "OpenAI.Codex_2p2nqsd0c76g0!App",
        [],
        BTreeMap::new(),
    )
    .is_err());

    let mut environment = BTreeMap::new();
    environment.insert("CODEX_HOME".to_string(), OsString::from("bad\0home"));
    assert!(WindowsActivationRequest::new(
        "OpenAI.Codex_26.820.7780.0_x64__2p2nqsd0c76g0",
        "OpenAI.Codex_2p2nqsd0c76g0!App",
        [],
        environment,
    )
    .is_err());
}
