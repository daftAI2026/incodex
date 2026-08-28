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
    let profile = after_confirmation
        .find("let profile =")
        .expect("install profile setup");
    assert!(
        rediscovery < profile,
        "the Store generation must be revalidated before state access"
    );
    assert!(
        revalidated.contains("&confirmed_app.package_full_name"),
        "the confirmed Store generation must be the one passed to mutation"
    );
}
