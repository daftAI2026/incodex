#![cfg(target_os = "windows")]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use incodex_cli::windows_app::{inspect_codex_package, parse_package_evidence};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "incodex-windows-app-{}-{sequence}",
        std::process::id()
    ))
}

fn package_fixture(root: &Path) {
    fs::create_dir_all(root.join("app")).expect("create app directory");
    fs::create_dir_all(root.join("tools")).expect("create tools directory");
    fs::write(root.join("AppxManifest.xml"), "<Package />").expect("write manifest");
    fs::write(root.join("app/ChatGPT.exe"), b"fixture").expect("write executable");
    fs::write(root.join("tools/Other.exe"), b"fixture").expect("write helper");
}

fn evidence_json(install_location: &Path) -> String {
    let path = install_location.display().to_string().replace('\\', "\\\\");
    format!(
        r#"{{"name":"OpenAI.Codex","packageFullName":"OpenAI.Codex_1.2.3.4_x64__2p2nqsd0c76g0","packageFamilyName":"OpenAI.Codex_2p2nqsd0c76g0","applications":[{{"applicationId":"Updater","applicationExecutable":"tools\\Other.exe"}},{{"applicationId":"App","applicationExecutable":"app\\ChatGPT.exe"}}],"installLocation":"{path}","architecture":"X64","signatureKind":"Store","status":"Ok"}}"#
    )
}

#[test]
fn accepts_healthy_store_evidence_with_the_real_electron_layout() {
    let root = scratch().join("程序 Files").join("OpenAI.Codex_1.2.3.4");
    package_fixture(&root);

    let evidence = parse_package_evidence(&evidence_json(&root)).expect("parse evidence");
    let app = inspect_codex_package(evidence).expect("inspect package");
    assert_eq!(
        app.install_location,
        root.canonicalize().expect("canonical package")
    );
    assert_eq!(app.executable, app.install_location.join("app/ChatGPT.exe"));
    assert_eq!(app.app_user_model_id, "OpenAI.Codex_2p2nqsd0c76g0!App");

    fs::remove_dir_all(root.parent().unwrap().parent().unwrap()).expect("remove fixture");
}

#[test]
fn accepts_a_launchable_store_package_without_unused_electron_internals() {
    let root = scratch().join("OpenAI.Codex_1.2.3.4");
    fs::create_dir_all(root.join("app")).expect("create package app directory");
    fs::write(root.join("AppxManifest.xml"), "<Package />").expect("write manifest");
    fs::write(root.join("app/ChatGPT.exe"), b"fixture").expect("write executable");

    let evidence = parse_package_evidence(&evidence_json(&root)).expect("parse evidence");
    let app = inspect_codex_package(evidence).expect("launchable package remains available");
    assert_eq!(app.executable, app.install_location.join("app/ChatGPT.exe"));

    fs::remove_dir_all(root.parent().unwrap()).expect("remove fixture");
}

#[test]
fn rejects_an_app_user_model_id_outside_the_official_package_family() {
    let root = scratch();
    let json =
        evidence_json(&root).replace("OpenAI.Codex_2p2nqsd0c76g0", "Impostor.Codex_2p2nqsd0c76g0");
    let evidence = parse_package_evidence(&json).expect("parse evidence");

    let error = inspect_codex_package(evidence).unwrap_err();
    assert!(error.contains("package identity"), "{error}");
    assert!(!root.exists(), "inspection created package state");
}

#[test]
fn rejects_untrusted_package_identity_before_using_its_path() {
    let root = scratch();
    let json = evidence_json(&root).replace("\"Store\"", "\"Developer\"");
    let evidence = parse_package_evidence(&json).expect("parse evidence");

    let error = inspect_codex_package(evidence).unwrap_err();
    assert!(error.contains("Microsoft Store"), "{error}");
    assert!(!root.exists(), "inspection created package state");
}

#[test]
fn rejects_an_aumid_whose_manifest_application_points_to_another_executable() {
    let root = scratch();
    package_fixture(&root);
    let json = evidence_json(&root).replace("app\\\\ChatGPT.exe", "app\\\\Other.exe");
    let evidence = parse_package_evidence(&json).expect("parse evidence");

    let error = inspect_codex_package(evidence).unwrap_err();
    assert!(error.contains("application executable"), "{error}");

    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn rejects_ambiguous_codex_applications_in_one_manifest() {
    let root = scratch();
    package_fixture(&root);
    let json = evidence_json(&root).replace(
        r#"{"applicationId":"App","applicationExecutable":"app\\ChatGPT.exe"}]"#,
        r#"{"applicationId":"App","applicationExecutable":"app\\ChatGPT.exe"},{"applicationId":"Second","applicationExecutable":"app\\ChatGPT.exe"}]"#,
    );
    let evidence = parse_package_evidence(&json).expect("parse evidence");

    let error = inspect_codex_package(evidence).unwrap_err();
    assert!(error.contains("exactly one Codex application"), "{error}");

    fs::remove_dir_all(root).expect("remove fixture");
}
