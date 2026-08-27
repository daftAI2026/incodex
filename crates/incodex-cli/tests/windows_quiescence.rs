#![cfg(target_os = "windows")]

#[test]
fn install_and_uninstall_request_normal_package_exit_before_mutation() {
    let source = include_str!("../src/windows_install.rs");
    let install = source
        .split("pub fn run_install")
        .nth(1)
        .expect("Windows install entry");
    let install_request = install
        .find("request_official_package_exit_and_wait")
        .expect("normal install exit request");
    let install_mutation = install
        .find("install_windows_runtime_with")
        .expect("install mutation");
    assert!(install_request < install_mutation);

    let uninstall = source
        .split("pub fn run_uninstall")
        .nth(1)
        .expect("Windows uninstall entry");
    let uninstall_request = uninstall
        .find("request_official_package_exit_and_wait")
        .expect("normal uninstall exit request");
    let uninstall_mutation = uninstall
        .find("uninstall_windows_runtime_approved_with")
        .expect("uninstall mutation");
    assert!(uninstall_request < uninstall_mutation);
}

#[test]
fn windows_requester_uses_restart_manager_without_force_or_window_close() {
    let source = include_str!("../src/windows_quiescence.rs");
    assert!(source.contains("RmShutdown"));
    assert!(source.contains("request_normal_exit_and_wait_with"));
    assert!(!source.contains("RmForceShutdown"));
    assert!(!source.contains("WM_CLOSE"));
    assert!(!source.contains("TerminateProcess"));
    assert!(!source.contains("TerminateAllProcesses"));
}
