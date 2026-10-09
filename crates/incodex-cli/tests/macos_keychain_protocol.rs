#![cfg(target_os = "macos")]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use incodex_cli::macos_keychain_protocol::{
    is_allowed_query, is_authorized_caller, parse_helper_response, CallerIdentity, HELPER_TIMEOUT,
    KEYCHAIN_ACCOUNT, KEYCHAIN_SERVICE, MAX_HELPER_OUTPUT_BYTES,
};

fn scratch() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "incodex-macos-keychain-protocol-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn valid_caller(root: &Path) -> (PathBuf, CallerIdentity) {
    let executable = root.join("Applications/ChatGPT.app/Contents/MacOS/ChatGPT");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, b"synthetic host").unwrap();
    let executable = fs::canonicalize(executable).unwrap();
    let uid = unsafe { libc::geteuid() as u32 };
    let caller = CallerIdentity {
        real_uid: uid,
        effective_uid: uid,
        parent_uid: uid,
        parent_executable: executable.clone(),
        parent_identifier: "com.openai.codex".into(),
    };
    (executable, caller)
}

#[test]
fn keychain_request_accepts_only_the_fixed_storage_item() {
    assert!(is_allowed_query(
        KEYCHAIN_SERVICE,
        KEYCHAIN_ACCOUNT,
        true,
        true,
    ));
    assert!(!is_allowed_query(
        "Other Storage Key",
        KEYCHAIN_ACCOUNT,
        true,
        true,
    ));
    assert!(!is_allowed_query(
        KEYCHAIN_SERVICE,
        "Other Account",
        true,
        true,
    ));
    assert!(!is_allowed_query(
        KEYCHAIN_SERVICE,
        KEYCHAIN_ACCOUNT,
        false,
        true,
    ));
    assert!(!is_allowed_query(
        KEYCHAIN_SERVICE,
        KEYCHAIN_ACCOUNT,
        true,
        false,
    ));
}

#[test]
fn caller_must_match_uid_direct_parent_canonical_path_and_identifier() {
    let root = scratch();
    let (executable, valid) = valid_caller(&root);
    assert!(is_authorized_caller(
        &valid,
        &executable,
        "com.openai.codex"
    ));

    let mut wrong_real_uid = valid.clone();
    wrong_real_uid.real_uid = wrong_real_uid.real_uid.saturating_add(1);
    assert!(!is_authorized_caller(
        &wrong_real_uid,
        &executable,
        "com.openai.codex"
    ));

    let mut wrong_effective_uid = valid.clone();
    wrong_effective_uid.effective_uid = wrong_effective_uid.effective_uid.saturating_add(1);
    assert!(!is_authorized_caller(
        &wrong_effective_uid,
        &executable,
        "com.openai.codex"
    ));

    let mut wrong_parent_uid = valid.clone();
    wrong_parent_uid.parent_uid = wrong_parent_uid.parent_uid.saturating_add(1);
    assert!(!is_authorized_caller(
        &wrong_parent_uid,
        &executable,
        "com.openai.codex"
    ));

    let mut wrong_path = valid.clone();
    wrong_path.parent_executable = root.join("Terminal.app/Contents/MacOS/Terminal");
    assert!(!is_authorized_caller(
        &wrong_path,
        &executable,
        "com.openai.codex"
    ));

    let mut wrong_identifier = valid;
    wrong_identifier.parent_identifier = "com.example.attacker".into();
    assert!(!is_authorized_caller(
        &wrong_identifier,
        &executable,
        "com.openai.codex"
    ));
}

#[test]
fn helper_response_is_bounded_and_the_timeout_is_two_seconds() {
    assert_eq!(HELPER_TIMEOUT, Duration::from_secs(2));
    assert_eq!(
        parse_helper_response(b"synthetic-key").unwrap(),
        b"synthetic-key"
    );
    assert!(parse_helper_response(&vec![b'x'; MAX_HELPER_OUTPUT_BYTES + 1]).is_err());
    assert!(parse_helper_response(&[]).is_err());
}
