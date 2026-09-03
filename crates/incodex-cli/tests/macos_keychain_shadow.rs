#![cfg(target_os = "macos")]

//! Shadow Keychain migration contract.
//!
//! These tests deliberately exercise only pure Rust policy and source contracts.  They do
//! not invoke Security.framework, read the user's Keychain, or carry a real storage key.

use std::path::PathBuf;

use incodex_cli::macos_keychain_assets::{
    ensure_registration, promote_authorization_from_proof, read_registration,
    should_install_keychain_provider, KeychainRegistration,
};
use incodex_cli::macos_keychain_protocol::{
    is_allowed_query, shadow_bridge_status_from_exit_code, ShadowBridgeStatus, KEYCHAIN_ACCOUNT,
    KEYCHAIN_SERVICE, SHADOW_KEYCHAIN_ACCOUNT, SHADOW_KEYCHAIN_SERVICE,
};

fn registration(authorization_ready: bool) -> KeychainRegistration {
    KeychainRegistration {
        schema_version: 1,
        app_path: PathBuf::from("/synthetic/ChatGPT.app"),
        helper_path: PathBuf::from("/synthetic/helpers/incodex-keychain-helper"),
        helper_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        authorization_ready,
    }
}

#[test]
fn shadow_item_uses_an_independent_namespace_without_widening_the_official_interceptor() {
    assert!(!SHADOW_KEYCHAIN_SERVICE.is_empty());
    assert!(!SHADOW_KEYCHAIN_ACCOUNT.is_empty());
    assert_ne!(SHADOW_KEYCHAIN_SERVICE, KEYCHAIN_SERVICE);
    assert_ne!(SHADOW_KEYCHAIN_ACCOUNT, KEYCHAIN_ACCOUNT);

    // The shadow item is an internal migration source.  It must never make the
    // production provider intercept an arbitrary service/account pair.
    assert!(!is_allowed_query(
        SHADOW_KEYCHAIN_SERVICE,
        SHADOW_KEYCHAIN_ACCOUNT,
        true,
        true,
    ));
}

#[test]
fn shadow_bridge_exposes_status_only_and_never_a_key_payload() {
    // The bridge boundary is deliberately an exit-status mapping, not a byte
    // transport.  This test therefore has no secret-shaped fixture at all.
    let cases = [
        (Some(0), ShadowBridgeStatus::Ready),
        (Some(44), ShadowBridgeStatus::ItemMissing),
        (Some(68), ShadowBridgeStatus::NotAuthorized),
        (Some(70), ShadowBridgeStatus::Unavailable),
    ];
    for (exit_code, expected) in cases {
        assert_eq!(
            shadow_bridge_status_from_exit_code(exit_code).unwrap(),
            expected
        );
    }
    assert!(shadow_bridge_status_from_exit_code(None).is_err());
}

#[test]
fn provider_install_requires_durable_foreground_readiness() {
    assert!(!should_install_keychain_provider(None));
    assert!(!should_install_keychain_provider(Some(&registration(
        false
    ))));
    assert!(should_install_keychain_provider(Some(&registration(true))));
}

#[test]
fn background_recovery_never_enters_the_interactive_authorization_path() {
    let install = include_str!("../src/install.rs");
    let function = install
        .split("fn install_app_for_expected_build")
        .nth(1)
        .expect("the native install path must remain explicit");
    let explicit_gate = function
        .find("if expected_build.is_none()")
        .expect("foreground authorization must have an explicit-install gate");
    let authorization = function
        .find("authorize_registration")
        .expect("the foreground path must retain its authorization operation");
    assert!(
        explicit_gate < authorization,
        "background update recovery must not open a Keychain password prompt"
    );

    let provider = function
        .find("install_keychain_provider")
        .expect("the native install path must retain provider placement");
    let provider_block = &function[provider.saturating_sub(180)..provider];
    assert!(
        provider_block.contains("authorization_ready"),
        "an unready registration must skip provider installation"
    );
}

#[test]
fn matching_noninteractive_runtime_proof_promotes_the_same_helper_generation() {
    let home = std::env::temp_dir().join(format!(
        "incodex-keychain-proof-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&home);
    let root = home.join(".incodex");
    let app = home.join("Applications/ChatGPT.app");
    let source = home.join("helper");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(&source, b"fixed-helper").unwrap();
    let registration = ensure_registration(&root, &app, &source).unwrap();
    assert!(!registration.authorization_ready);

    let proof_path = root.join("macos-keychain/authorization-proof.json");
    std::fs::write(
        &proof_path,
        format!(
            "{}\n",
            serde_json::json!({
                "schemaVersion": 1,
                "appPath": registration.app_path,
                "helperPath": registration.helper_path,
                "helperSha256": registration.helper_sha256,
            })
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&proof_path, std::fs::Permissions::from_mode(0o600)).unwrap();

    assert!(promote_authorization_from_proof(&root).unwrap());
    assert!(read_registration(&root)
        .unwrap()
        .unwrap()
        .authorization_ready);
    assert!(!proof_path.exists());
    std::fs::remove_dir_all(home).unwrap();
}
