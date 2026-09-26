#![cfg(target_os = "macos")]

use incodex_macos::{read_entitlements, sign_app_with_asar_integrity, verify_bundle_deep_strict};
use std::{
    fs,
    path::Path,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

fn run(program: &str, args: &[&str]) {
    let output = Command::new(program).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{program}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn write_plist(path: &Path, executable: &str, identifier: &str, extra: &str) {
    fs::write(path, format!(r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>{executable}</string><key>CFBundleIdentifier</key><string>{identifier}</string><key>CFBundleVersion</key><string>1</string>{extra}</dict></plist>"#)).unwrap();
}

struct SignedFixture {
    root: std::path::PathBuf,
    app: std::path::PathBuf,
    framework: std::path::PathBuf,
    binary: std::path::PathBuf,
    helper: std::path::PathBuf,
    helper_binary: std::path::PathBuf,
}

fn signed_fixture(helper_identifier: &str) -> SignedFixture {
    signed_fixture_with_loader(helper_identifier, false)
}

fn signed_fixture_with_loader(helper_identifier: &str, dynamic: bool) -> SignedFixture {
    signed_fixture_with_loader_location(helper_identifier, dynamic, false)
}

fn signed_fixture_with_loader_location(
    helper_identifier: &str,
    dynamic: bool,
    sibling_helper: bool,
) -> SignedFixture {
    signed_fixture_with_linkage(
        helper_identifier,
        dynamic,
        sibling_helper,
        false,
        false,
        false,
    )
}

fn signed_fixture_with_app_descendant_loader(helper_identifier: &str) -> SignedFixture {
    signed_fixture_with_linkage(helper_identifier, false, false, true, false, false)
}

fn signed_fixture_with_rpath_loader(
    helper_identifier: &str,
    sibling_helper: bool,
    shadow_first_rpath: bool,
) -> SignedFixture {
    signed_fixture_with_linkage(
        helper_identifier,
        false,
        sibling_helper,
        false,
        true,
        shadow_first_rpath,
    )
}

fn signed_fixture_with_linkage(
    helper_identifier: &str,
    dynamic: bool,
    sibling_helper: bool,
    app_descendant_helper: bool,
    rpath_link: bool,
    shadow_first_rpath: bool,
) -> SignedFixture {
    signed_fixture_with_linkage_options(
        helper_identifier,
        dynamic,
        sibling_helper,
        app_descendant_helper,
        rpath_link,
        shadow_first_rpath,
        false,
    )
}

fn signed_fixture_with_absolute_loader(helper_identifier: &str) -> SignedFixture {
    signed_fixture_with_linkage_options(helper_identifier, false, true, false, false, false, true)
}

fn signed_fixture_with_linkage_options(
    helper_identifier: &str,
    dynamic: bool,
    sibling_helper: bool,
    app_descendant_helper: bool,
    rpath_link: bool,
    shadow_first_rpath: bool,
    absolute_load_path: bool,
) -> SignedFixture {
    let root = std::env::temp_dir().join(format!(
        "incodex-integrity-signing-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let app = root.join("ChatGPT.app");
    let framework = app.join("Contents/Frameworks/Renamed.framework");
    fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    fs::create_dir_all(framework.join("Resources")).unwrap();
    let digest = "6f22b7a4f82a2d9f48798c779ac3eec57d2cf91e549ce42866b193dd2ea3ec67";
    let bytes = (0..64)
        .step_by(2)
        .map(|i| format!("0x{}", &digest[i..i + 2]))
        .collect::<Vec<_>>()
        .join(",");
    let source = root.join("framework.c");
    fs::write(&source, format!(r#"__attribute__((used,section("__DATA_CONST,__asar_integrity"))) const struct {{ char sentinel[32]; unsigned char used, version, digest[32]; }} slot = {{"AGbevlPCksUGKNL8TSn7wGmJEuJsXb2A", 1, 1, {{{bytes}}}}}; int fixture(void) {{return 1;}}"#)).unwrap();
    let binary = framework.join("Renamed");
    let absolute_install_name = format!("-Wl,-install_name,{}", binary.display());
    let mut framework_args = vec!["-dynamiclib", source.to_str().unwrap()];
    if absolute_load_path {
        framework_args.push(&absolute_install_name);
    } else if rpath_link {
        framework_args.push("-Wl,-install_name,@rpath/Renamed.framework/Renamed");
    }
    framework_args.extend(["-o", binary.to_str().unwrap()]);
    run("clang", &framework_args);
    let main = root.join("main.c");
    fs::write(&main, "int main(void) {return 0;}").unwrap();
    run(
        "clang",
        &[
            main.to_str().unwrap(),
            "-o",
            app.join("Contents/MacOS/ChatGPT").to_str().unwrap(),
        ],
    );
    write_plist(
        &framework.join("Resources/Info.plist"),
        "Renamed",
        "com.openai.codex.framework",
        "",
    );
    if shadow_first_rpath {
        assert!(rpath_link && sibling_helper);
        let shadow = app.join("Contents/Frameworks/Mask/Renamed.framework");
        fs::create_dir_all(shadow.join("Resources")).unwrap();
        let shadow_source = root.join("shadow.c");
        fs::write(&shadow_source, "int fixture(void) { return 1; }").unwrap();
        run(
            "clang",
            &[
                "-dynamiclib",
                shadow_source.to_str().unwrap(),
                "-Wl,-install_name,@rpath/Renamed.framework/Renamed",
                "-o",
                shadow.join("Renamed").to_str().unwrap(),
            ],
        );
        write_plist(
            &shadow.join("Resources/Info.plist"),
            "Renamed",
            "com.openai.codex.framework.mask",
            "",
        );
        run(
            "codesign",
            &["--force", "--sign", "-", shadow.to_str().unwrap()],
        );
    }
    let integrity = format!("<key>ElectronAsarIntegrity</key><dict><key>Resources/app.asar</key><dict><key>algorithm</key><string>SHA256</string><key>hash</key><string>{}</string></dict></dict>", "a".repeat(64));
    write_plist(
        &app.join("Contents/Info.plist"),
        "ChatGPT",
        "com.openai.codex",
        &integrity,
    );
    let helper = if app_descendant_helper {
        app.join("Contents/Helpers/LinkedHelper.app")
    } else if sibling_helper {
        app.join("Contents/Frameworks/LinkedHelper.app")
    } else {
        framework.join("Helpers/LinkedHelper.app")
    };
    fs::create_dir_all(helper.join("Contents/MacOS")).unwrap();
    write_plist(
        &helper.join("Contents/Info.plist"),
        "LinkedHelper",
        helper_identifier,
        "",
    );
    let helper_source = root.join("helper.c");
    let source = if dynamic {
        r#"#include <dlfcn.h>
#include <mach-o/dyld.h>
#include <libgen.h>
#include <stdio.h>
int main(void) { char exe[4096], path[8192]; uint32_t size=sizeof(exe);
if (_NSGetExecutablePath(exe,&size)) return 1;
snprintf(path,sizeof(path),"%s/%s",dirname(exe),"../../../../Renamed");
void *handle=dlopen(path,RTLD_LAZY); if (!handle) return 2;
return dlsym(handle,"ChromeMain") ? 0 : 3; }
"#
    } else {
        "extern int fixture(void); int main(void) {return fixture()-1;}"
    };
    fs::write(&helper_source, source).unwrap();
    let helper_binary = helper.join("Contents/MacOS/LinkedHelper");
    let mut args = vec![helper_source.to_str().unwrap()];
    if rpath_link {
        args.extend(["-F", framework.parent().unwrap().to_str().unwrap()]);
        args.extend(["-framework", "Renamed"]);
        if shadow_first_rpath {
            args.push("-Wl,-rpath,@loader_path/../../../Mask");
        }
        args.push(if sibling_helper {
            "-Wl,-rpath,@loader_path/../../../"
        } else {
            "-Wl,-rpath,@loader_path/../../../../../"
        });
    } else if !dynamic {
        args.push(binary.to_str().unwrap());
    }
    args.extend(["-o", helper_binary.to_str().unwrap()]);
    run("clang", &args);
    let entitlements = root.join("helper-entitlements.plist");
    fs::write(&entitlements, r#"<?xml version="1.0"?><plist><dict><key>com.apple.security.cs.allow-jit</key><true/></dict></plist>"#).unwrap();
    run(
        "codesign",
        &[
            "--force",
            "--sign",
            "-",
            "--options",
            "runtime",
            "--entitlements",
            entitlements.to_str().unwrap(),
            helper.to_str().unwrap(),
        ],
    );
    run(
        "codesign",
        &["--force", "--sign", "-", framework.to_str().unwrap()],
    );
    run(
        "codesign",
        &["--force", "--sign", "-", app.to_str().unwrap()],
    );
    let helper_binary = helper.join("Contents/MacOS/LinkedHelper");
    SignedFixture {
        root,
        app,
        framework,
        binary,
        helper,
        helper_binary,
    }
}

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

    sign_app_with_asar_integrity(&staged, &"c".repeat(64)).unwrap();
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
