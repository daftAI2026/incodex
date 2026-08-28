#[test]
fn install_revalidates_store_generation_after_confirmation() {
    let source = include_str!("../src/windows_install.rs");
    let install = source
        .split("pub fn run_install")
        .nth(1)
        .and_then(|source| source.split("pub fn install_windows_runtime_with").next())
        .expect("Windows install entry point");
    let confirmation = install
        .find("crate::confirm::require(\"install\", parsed.yes)?;")
        .expect("install confirmation");
    let after_confirmation = &install[confirmation..];
    let rediscovery = after_confirmation
        .find("let confirmed_app = discover_codex_package()?;")
        .expect("Store package must be rediscovered after confirmation");
    let revalidated = &after_confirmation[rediscovery..];
    assert!(
        revalidated.contains("confirmed_app.package_full_name != app.package_full_name"),
        "the post-confirmation Store generation must be compared with the displayed plan"
    );
    let gate = after_confirmation
        .find("let _registration_gate = acquire_windows_install_state()?;")
        .expect("install registration gate");
    assert!(
        gate < rediscovery,
        "the Store generation must be revalidated after the registration gate"
    );
    assert!(
        revalidated.contains("&confirmed_app.package_full_name"),
        "the confirmed Store generation must be the one passed to mutation"
    );
}

#[test]
fn install_revalidates_store_generation_at_the_locked_mutation_boundary() {
    let source = include_str!("../src/windows_install.rs");
    let mutation = source
        .split("fn install_windows_runtime_with_package_probe")
        .nth(1)
        .expect("install mutation probe boundary");
    let acquired = mutation
        .find("let _transaction = acquire_windows_install_state()?;")
        .expect("install mutation gate");
    let first_probe = mutation
        .find("revalidate_windows_install_generation")
        .expect("locked install generation probe");
    assert!(
        acquired < first_probe,
        "Store generation must be checked only after the mutation gate"
    );
    assert!(
        mutation.contains("WindowsInstallPhase::RecoveryRequired"),
        "a generation race must retain recoverable install state"
    );
    assert!(
        mutation.contains("disable(package_full_name)"),
        "a generation race after enable must roll back the old registration"
    );
}
