#![cfg(target_os = "windows")]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use incodex_cli::windows_activation::{
    activate_packaged_kill_on_drop, activate_packaged_with_installed_runtime,
    disable_installed_runtime, enable_installed_runtime, try_run_installed_package_debugger,
    try_run_package_debugger, windows_debugger_route, WindowsActivationFailure,
    WindowsActivationRequest, WindowsDebuggerRoute, WindowsInstalledRuntimeRegistration,
};
use incodex_cli::windows_launch::WindowsLaunchMode;

#[test]
fn stable_debugger_routes_only_an_isolated_profile_into_the_matching_job() {
    assert_eq!(
        windows_debugger_route("ChatGPT.exe codex://new?mode=codex").expect("normal route"),
        WindowsDebuggerRoute::ResumeNormally
    );
    let user_data_dir = r"C:\Users\Linus Torvalds\.incodex\sessions\one\chromium";
    let request = WindowsActivationRequest::new(
        "OpenAI.Codex_26.820.7780.0_x64__2p2nqsd0c76g0",
        "OpenAI.Codex_2p2nqsd0c76g0!App",
        [OsString::from(format!("--user-data-dir={user_data_dir}"))],
        BTreeMap::new(),
    )
    .expect("isolated activation request");
    let capability = request
        .activation_capability()
        .expect("derive activation capability from the isolated profile");
    assert_eq!(
        windows_debugger_route(&format!("ChatGPT.exe {}", request.arguments()))
            .expect("isolated route"),
        WindowsDebuggerRoute::AssignToJob(capability.job_name().to_string())
    );
}
use incodex_cli::windows_process::WindowsProcessTree;

#[test]
fn exposes_one_packaged_activation_backend_for_the_open_lifecycle() {
    let _backend: fn(
        &WindowsActivationRequest,
    ) -> Result<WindowsProcessTree, WindowsActivationFailure> = activate_packaged_kill_on_drop;
    let _installed_backend: fn(
        &WindowsActivationRequest,
        &WindowsInstalledRuntimeRegistration,
    ) -> Result<WindowsProcessTree, WindowsActivationFailure> =
        activate_packaged_with_installed_runtime;
}

#[test]
fn binds_the_persistent_debugger_to_a_stable_helper_state_and_package() {
    let package = "OpenAI.Codex_26.820.7780.0_x64__2p2nqsd0c76g0";
    let helper = Path::new(r"C:\Users\Linus Torvalds\.incodex\windows\helper.exe");
    let mut environment = BTreeMap::new();
    environment.insert(
        "NODE_OPTIONS".to_string(),
        OsString::from(r"--require=C:\Users\Linus Torvalds\.incodex\runtime\bootstrap.cjs"),
    );
    let registration = WindowsInstalledRuntimeRegistration::new(package, helper, environment)
        .expect("valid installed Runtime registration");

    assert_eq!(registration.package_full_name(), package);
    assert_eq!(
        registration.debugger_command_line(),
        r#""C:\Users\Linus Torvalds\.incodex\windows\helper.exe" __incodex_windows_installed_debugger"#
    );
    assert!(registration.debugger_command_line().len() < 260);
    assert_eq!(registration.environment().last(), Some(&0));
    assert_eq!(
        registration.environment()[registration.environment().len() - 2],
        0
    );
    let _enable: fn(&WindowsInstalledRuntimeRegistration) -> Result<(), String> =
        enable_installed_runtime;
    let _disable: fn(&str) -> Result<(), String> = disable_installed_runtime;
}

#[test]
fn installed_debugger_mode_refuses_to_resume_an_unowned_process() {
    let args = vec![
        "__incodex_windows_installed_debugger".to_string(),
        "--package".to_string(),
        "OpenAI.Codex_1.2.3.4_x64__publisher".to_string(),
        "--state".to_string(),
        r"C:\Users\test\.incodex\windows-install.json".to_string(),
        "-p".to_string(),
        std::process::id().to_string(),
        "-tid".to_string(),
        u32::MAX.to_string(),
    ];
    let result = try_run_installed_package_debugger(&args).expect("recognize helper mode");
    assert!(result.is_err());
}

#[test]
fn transient_debugger_requires_the_exact_package_identity() {
    let args = vec![
        "__incodex_windows_package_debugger".to_string(),
        "--job".to_string(),
        "Local\\Incodex-test-job".to_string(),
        "-p".to_string(),
        std::process::id().to_string(),
        "-tid".to_string(),
        u32::MAX.to_string(),
    ];
    let error = try_run_package_debugger(&args)
        .expect("recognize helper mode")
        .expect_err("reject package-less debugger callback");
    assert!(error.contains("--package"));
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
fn installed_activation_moves_isolation_into_one_authenticated_capability() {
    let user_home = r"C:\Users\林 纳斯\.incodex\sessions\one\codex-home";
    let request = WindowsActivationRequest::new(
        "OpenAI.Codex_26.820.7780.0_x64__2p2nqsd0c76g0",
        "OpenAI.Codex_2p2nqsd0c76g0!App",
        [OsString::from(
            r"--user-data-dir=C:\Users\林 纳斯\.incodex\sessions\one\chromium\",
        )],
        BTreeMap::from([
            ("CODEX_HOME".to_string(), OsString::from(user_home)),
            ("INCODEX_INCOGNITO".to_string(), OsString::from("1")),
        ]),
    )
    .expect("valid installed activation request");

    let capability = request
        .activation_capability()
        .expect("derive capability from the isolated profile");
    let arguments = request.arguments();
    assert!(arguments
        .starts_with(r#""--user-data-dir=C:\Users\林 纳斯\.incodex\sessions\one\chromium\\""#,));
    assert_eq!(arguments.matches("--incodex-activation-token=").count(), 0);
    assert_eq!(
        windows_debugger_route(&format!("ChatGPT.exe {arguments}"))
            .expect("route isolated profile"),
        WindowsDebuggerRoute::AssignToJob(capability.job_name().to_string())
    );

    let claimed = request
        .activation_environment(WindowsLaunchMode::Runtime)
        .expect("build authenticated environment response");
    assert_eq!(claimed.mode, WindowsLaunchMode::Runtime);
    assert_eq!(
        claimed.environment.get("CODEX_HOME").map(String::as_str),
        Some(user_home)
    );
    assert_eq!(
        claimed
            .environment
            .get("INCODEX_INCOGNITO")
            .map(String::as_str),
        Some("1")
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
