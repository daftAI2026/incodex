#![cfg(target_os = "windows")]

#[test]
fn windows_mutation_requires_the_user_to_quit_codex_without_requesting_exit() {
    let install = include_str!("../src/windows_install.rs");

    assert!(!install.contains("request_official_package_exit_and_wait"));
    assert!(install.contains("running_package_process_ids"));
    assert!(install.contains("close Codex before installing the Windows Runtime"));
    assert!(install.contains("close Codex to finish uninstalling the Windows Runtime"));
}

#[test]
fn windows_runtime_never_exposes_an_automatic_official_app_quit_path() {
    let platform = include_str!("../assets/incodex-windows-platform.cjs");
    let runtime = include_str!("../../../src/runtime/incodex-main.cts");

    for forbidden in [
        "listenForNormalExit",
        "Incodex-Runtime-Control-",
        "CallNamedPipeW",
        "RmShutdown",
        "WM_CLOSE",
        "TerminateProcess",
        "TerminateAllProcesses",
    ] {
        assert!(!platform.contains(forbidden), "platform contains {forbidden}");
        assert!(!runtime.contains(forbidden), "Runtime contains {forbidden}");
    }
}
