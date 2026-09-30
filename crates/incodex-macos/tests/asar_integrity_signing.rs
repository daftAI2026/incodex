#![cfg(target_os = "macos")]

#[path = "support/asar_integrity_fixture.rs"]
mod fixture;
use fixture::*;
use incodex_macos::{
    read_entitlements, sign_app_with_asar_integrity, sign_staged_app_with_asar_integrity,
    verify_bundle_deep_strict,
};
use std::{fs, path::Path, process::Command};

fn code_directory_hash(path: &Path) -> String {
    let output = Command::new("codesign")
        .args(["--display", "--verbose=4", "--"])
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "codesign display {}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    let display = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    display
        .lines()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("CDHash=")
                .or_else(|| line.strip_prefix("CandidateCDHashFull sha256="))
                .or_else(|| line.strip_prefix("CandidateCDHash sha256="))
        })
        .expect("codesign output contains a CDHash")
        .to_owned()
}

fn assert_signed_framework_digest_matches_updated_plist(helper_identifier: &str) {
    let fixture = signed_fixture(helper_identifier);
    sign_app_with_asar_integrity(&fixture.app, &"c".repeat(64)).unwrap();
    verify_bundle_deep_strict(&fixture.app).unwrap();
    let helper_entitlements = read_entitlements(&fixture.helper).unwrap();
    assert!(
        helper_entitlements
            .keys
            .contains("com.apple.security.cs.allow-jit"),
        "retain helper's original entitlements"
    );
    assert!(helper_entitlements.keys.contains("com.apple.security.cs.disable-library-validation"), "a hardened helper loading the newly ad-hoc framework needs its own library-validation entitlement");
    let actual = fs::read(&fixture.binary).unwrap();
    let marker = b"AGbevlPCksUGKNL8TSn7wGmJEuJsXb2A";
    let offset = actual
        .windows(marker.len())
        .position(|slice| slice == marker)
        .unwrap();
    let slot = &actual[offset + 32..offset + 66];
    fs::remove_dir_all(&fixture.root).unwrap();
    assert_eq!(
        &slot[..2],
        &[1, 1],
        "validation must remain enabled/version 1"
    );
    let expected = "aab91287495cc7a72a1a95c182a0a8be1025e4e8a8057d9812a743981bac2b62";
    let actual_digest = slot[2..]
        .iter()
        .map(|value| format!("{value:02x}"))
        .collect::<String>();
    assert_eq!(
        actual_digest, expected,
        "deep codesign success does not prove Electron integrity consistency"
    );
}

#[test]
fn framework_alert_notification_service_identity_is_supported() {
    assert_signed_framework_digest_matches_updated_plist(
        "com.openai.codex.framework.AlertNotificationService",
    );
}

#[test]
fn codex_helper_namespace_identity_remains_supported() {
    assert_signed_framework_digest_matches_updated_plist("com.openai.codex.helper.fixture");
}

#[test]
fn dynamic_framework_loader_retains_its_own_entitlements_and_library_validation_exemption() {
    let fixture = signed_fixture_with_loader("com.openai.codex.helper.renderer", true);
    sign_app_with_asar_integrity(&fixture.app, &"c".repeat(64)).unwrap();
    verify_bundle_deep_strict(&fixture.app).unwrap();
    let entitlements = read_entitlements(&fixture.helper).unwrap();
    assert!(
        entitlements
            .keys
            .contains("com.apple.security.cs.disable-library-validation"),
        "a dlopen helper needs its own exemption for the modified Framework"
    );
    assert!(entitlements
        .keys
        .contains("com.apple.security.cs.allow-jit"));
    fs::remove_dir_all(&fixture.root).unwrap();
}

#[test]
fn sibling_direct_dependent_helper_gets_resigned_with_library_validation_exemption() {
    let fixture =
        signed_fixture_with_loader_location("com.openai.codex.helper.linked", false, true);
    sign_app_with_asar_integrity(&fixture.app, &"c".repeat(64)).unwrap();
    verify_bundle_deep_strict(&fixture.app).unwrap();
    let entitlements = read_entitlements(&fixture.helper).unwrap();
    assert!(entitlements
        .keys
        .contains("com.apple.security.cs.disable-library-validation"));
    assert!(entitlements
        .keys
        .contains("com.apple.security.cs.allow-jit"));
    fs::remove_dir_all(&fixture.root).unwrap();
}

#[test]
fn absolute_final_app_framework_load_is_resolved_to_its_staged_copy() {
    let fixture = signed_fixture_with_absolute_loader("com.openai.codex.helper.absolute");
    let staged = fixture.root.join("scratch/ChatGPT.app");
    fs::create_dir_all(staged.parent().unwrap()).unwrap();
    run(
        "ditto",
        &[fixture.app.to_str().unwrap(), staged.to_str().unwrap()],
    );

    let staged_helper_binary =
        staged.join("Contents/Frameworks/LinkedHelper.app/Contents/MacOS/LinkedHelper");
    let linkage = Command::new("otool")
        .args(["-L"])
        .arg(&staged_helper_binary)
        .output()
        .unwrap();
    assert!(linkage.status.success());
    let linkage = String::from_utf8_lossy(&linkage.stdout);
    assert!(
        linkage.contains(fixture.binary.to_str().unwrap()),
        "fixture must retain its absolute final-app load path: {linkage}"
    );

    sign_staged_app_with_asar_integrity(&staged, &fixture.app, &"c".repeat(64)).unwrap();
    verify_bundle_deep_strict(&staged).unwrap();
    let staged_helper = staged.join("Contents/Frameworks/LinkedHelper.app");
    let entitlements = read_entitlements(&staged_helper).unwrap();
    assert!(
        entitlements
            .keys
            .contains("com.apple.security.cs.disable-library-validation"),
        "helper loading the final app's changed Framework must be selected through the staged counterpart"
    );
    assert!(entitlements
        .keys
        .contains("com.apple.security.cs.allow-jit"));
    fs::remove_dir_all(&fixture.root).unwrap();
}

#[test]
fn external_absolute_framework_load_is_not_rebased_into_the_staged_app() {
    let fixture = signed_fixture_with_external_absolute_loader("com.openai.codex.helper.external");
    let staged = fixture.root.join("scratch/ChatGPT.app");
    fs::create_dir_all(staged.parent().unwrap()).unwrap();
    run(
        "ditto",
        &[fixture.app.to_str().unwrap(), staged.to_str().unwrap()],
    );
    let staged_helper_binary =
        staged.join("Contents/Frameworks/LinkedHelper.app/Contents/MacOS/LinkedHelper");
    let linkage = Command::new("otool")
        .args(["-L"])
        .arg(&staged_helper_binary)
        .output()
        .unwrap();
    assert!(linkage.status.success());
    let linkage = String::from_utf8_lossy(&linkage.stdout);
    assert!(
        linkage.contains(
            fixture
                .root
                .join("external/Unrelated.dylib")
                .to_str()
                .unwrap()
        ),
        "fixture must retain an external absolute load path: {linkage}"
    );

    sign_staged_app_with_asar_integrity(&staged, &fixture.app, &"c".repeat(64)).unwrap();
    verify_bundle_deep_strict(&staged).unwrap();
    let staged_helper = staged.join("Contents/Frameworks/LinkedHelper.app");
    let entitlements = read_entitlements(&staged_helper).unwrap();
    assert!(!entitlements
        .keys
        .contains("com.apple.security.cs.disable-library-validation"));
    fs::remove_dir_all(&fixture.root).unwrap();
}

#[test]
fn direct_dependent_helper_in_another_app_descendant_gets_resigned() {
    let fixture = signed_fixture_with_app_descendant_loader("com.openai.codex.helper.nested");
    sign_app_with_asar_integrity(&fixture.app, &"c".repeat(64)).unwrap();
    verify_bundle_deep_strict(&fixture.app).unwrap();
    let entitlements = read_entitlements(&fixture.helper).unwrap();
    assert!(entitlements
        .keys
        .contains("com.apple.security.cs.disable-library-validation"));
    fs::remove_dir_all(&fixture.root).unwrap();
}

#[test]
fn sibling_rpath_dependent_helper_gets_resigned_with_library_validation_exemption() {
    let fixture = signed_fixture_with_rpath_loader("com.openai.codex.helper.rpath", true, false);
    sign_app_with_asar_integrity(&fixture.app, &"c".repeat(64)).unwrap();
    verify_bundle_deep_strict(&fixture.app).unwrap();
    let entitlements = read_entitlements(&fixture.helper).unwrap();
    assert!(entitlements
        .keys
        .contains("com.apple.security.cs.disable-library-validation"));
    fs::remove_dir_all(&fixture.root).unwrap();
}

#[test]
fn earlier_existing_non_target_rpath_masks_later_target_framework() {
    let fixture = signed_fixture_with_rpath_loader("com.openai.codex.helper.rpath", true, true);
    sign_app_with_asar_integrity(&fixture.app, &"c".repeat(64)).unwrap();
    verify_bundle_deep_strict(&fixture.app).unwrap();
    let entitlements = read_entitlements(&fixture.helper).unwrap();
    assert!(!entitlements
        .keys
        .contains("com.apple.security.cs.disable-library-validation"));
    fs::remove_dir_all(&fixture.root).unwrap();
}

#[test]
fn sibling_unknown_or_cua_dependent_is_rejected_before_any_bundle_mutation() {
    for identifier in ["com.openai.sky.fixture", "com.openai.cua.fixture"] {
        let fixture = signed_fixture_with_loader_location(identifier, false, true);
        assert_rejection_unchanged(fixture, "unknown helper");
    }
}

#[test]
fn unknown_direct_dependent_is_rejected_before_any_bundle_mutation() {
    assert_unknown_dependent_unchanged(false);
}

#[test]
fn unknown_dynamic_dependent_is_rejected_before_any_bundle_mutation() {
    assert_unknown_dependent_unchanged(true);
}

fn assert_unknown_dependent_unchanged(dynamic: bool) {
    let fixture = signed_fixture_with_loader("com.openai.sky.fixture", dynamic);
    assert_rejection_unchanged(fixture, "unknown helper");
}

#[test]
fn helper_signed_identity_cannot_be_spoofed_by_its_plist() {
    assert_signed_identity_mismatch_unchanged("helper");
}

#[test]
fn framework_signed_identity_cannot_be_spoofed_by_its_plist() {
    assert_signed_identity_mismatch_unchanged("framework");
}

#[test]
fn host_signed_identity_cannot_be_spoofed_by_its_plist() {
    assert_signed_identity_mismatch_unchanged("host");
}

#[test]
fn planned_asar_resource_patch_does_not_invalidate_the_unmodified_host_code_identity() {
    let fixture = signed_fixture("com.openai.codex.helper.fixture");
    fs::create_dir_all(fixture.app.join("Contents/Resources")).unwrap();
    let asar = fixture.app.join("Contents/Resources/app.asar");
    fs::write(&asar, b"original archive fixture").unwrap();
    run(
        "codesign",
        &["--force", "--sign", "-", fixture.app.to_str().unwrap()],
    );
    verify_bundle_deep_strict(&fixture.app).unwrap();
    // The installer edits ASAR in the stage before it calls the signing API.
    fs::write(&asar, b"planned patched archive fixture").unwrap();
    assert!(verify_bundle_deep_strict(&fixture.app).is_err());
    run(
        "codesign",
        &[
            "--verify",
            "--strict",
            "--ignore-resources",
            fixture.app.to_str().unwrap(),
        ],
    );
    sign_app_with_asar_integrity(&fixture.app, &"c".repeat(64)).unwrap();
    verify_bundle_deep_strict(&fixture.app).unwrap();
    assert_eq!(fs::read(&asar).unwrap(), b"planned patched archive fixture");
    fs::remove_dir_all(&fixture.root).unwrap();
}

#[test]
fn host_code_corruption_still_fails_before_signing() {
    let fixture = signed_fixture("com.openai.codex.helper.fixture");
    let executable = fixture.app.join("Contents/MacOS/ChatGPT");
    let mut bytes = fs::read(&executable).unwrap();
    bytes[4096] ^= 1;
    fs::write(&executable, bytes).unwrap();
    assert_rejection_unchanged(fixture, "signature verification failed");
}

#[test]
fn host_plist_corruption_still_fails_even_when_resources_are_planned_to_change() {
    let fixture = signed_fixture("com.openai.codex.helper.fixture");
    let plist = fixture.app.join("Contents/Info.plist");
    let original = fs::read_to_string(&plist).unwrap();
    fs::write(
        &plist,
        original.replace("<string>1</string>", "<string>2</string>"),
    )
    .unwrap();
    let probe = Command::new("codesign")
        .args(["--verify", "--strict", "--ignore-resources"])
        .arg(&fixture.app)
        .output()
        .unwrap();
    assert!(
        !probe.status.success(),
        "Info.plist remains a sealed identity input"
    );
    assert_rejection_unchanged(fixture, "signature verification failed");
}

fn assert_signed_identity_mismatch_unchanged(role: &str) {
    let fixture = signed_fixture_with_loader("com.openai.codex.helper.renderer", true);
    if role == "helper" {
        run(
            "codesign",
            &[
                "--force",
                "--sign",
                "-",
                "--identifier",
                "com.openai.sky.fixture",
                "--options",
                "runtime",
                "--entitlements",
                fixture
                    .root
                    .join("helper-entitlements.plist")
                    .to_str()
                    .unwrap(),
                fixture.helper.to_str().unwrap(),
            ],
        );
    }
    run(
        "codesign",
        &[
            "--force",
            "--sign",
            "-",
            "--identifier",
            if role == "framework" {
                "com.openai.sky.framework"
            } else {
                "com.openai.codex.framework"
            },
            fixture.framework.to_str().unwrap(),
        ],
    );
    run(
        "codesign",
        &[
            "--force",
            "--sign",
            "-",
            "--identifier",
            if role == "host" {
                "com.openai.sky.host"
            } else {
                "com.openai.codex"
            },
            fixture.app.to_str().unwrap(),
        ],
    );
    verify_bundle_deep_strict(&fixture.app).unwrap();
    assert_rejection_unchanged(fixture, "signature identifier mismatch");
}

fn assert_rejection_unchanged(fixture: SignedFixture, expected_error: &str) {
    let info_plist = fixture.app.join("Contents/Info.plist");
    let original_plist = fs::read(&info_plist).unwrap();
    let host_binary = fixture.app.join("Contents/MacOS/ChatGPT");
    let original_host = fs::read(&host_binary).unwrap();
    let original_framework = fs::read(&fixture.binary).unwrap();
    let original_helper = fs::read(&fixture.helper_binary).unwrap();
    let original_hashes = [
        code_directory_hash(&fixture.app),
        code_directory_hash(&fixture.framework),
        code_directory_hash(&fixture.helper),
    ];

    let error = sign_app_with_asar_integrity(&fixture.app, &"c".repeat(64))
        .expect_err("unknown or mismatched dependent must fail closed");
    assert!(error.contains(expected_error), "{error}");
    assert_eq!(fs::read(&info_plist).unwrap(), original_plist);
    assert_eq!(fs::read(&host_binary).unwrap(), original_host);
    assert_eq!(fs::read(&fixture.binary).unwrap(), original_framework);
    assert_eq!(fs::read(&fixture.helper_binary).unwrap(), original_helper);
    assert_eq!(
        [
            code_directory_hash(&fixture.app),
            code_directory_hash(&fixture.framework),
            code_directory_hash(&fixture.helper),
        ],
        original_hashes,
        "rejection must not alter any original code signature"
    );
    fs::remove_dir_all(&fixture.root).unwrap();
}
