use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[path = "support/readonly.rs"]
mod readonly_support;
mod support;

use readonly_support::{isolated_home, parse_json, run};

fn sha256_hex(body: &[u8]) -> String {
    Sha256::digest(body)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn runtime_root(home: &Path) -> PathBuf {
    home.join(".incodex/runtime")
}

fn diagnostic_app(home: &Path) -> PathBuf {
    let app = home.join("ChatGPT.app");
    let asar = app.join("Contents/Resources/app.asar");
    fs::create_dir_all(asar.parent().unwrap()).unwrap();
    fs::write(asar, b"not an asar fixture\n").unwrap();
    app
}

fn read_current(home: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(runtime_root(home).join("current.json")).unwrap()).unwrap()
}

fn write_current(home: &Path, current: &serde_json::Value) {
    fs::write(
        runtime_root(home).join("current.json"),
        format!("{}\n", serde_json::to_string_pretty(current).unwrap()),
    )
    .unwrap();
}

fn publish_embedded_runtime(home: &Path) -> serde_json::Value {
    incodex_runtime_bundle::publish(&home.join(".incodex")).unwrap();
    read_current(home)
}

fn make_same_version_canonical_drift(home: &Path) -> (String, String) {
    let mut current = publish_embedded_runtime(home);
    let bundled_manifest = current["manifestSha256"].as_str().unwrap().to_string();
    let release = current["release"].as_str().unwrap();
    let old_release = runtime_root(home).join(release);

    let changed_body = b"same version, different Runtime content\n";
    fs::write(old_release.join("incodex-main.cjs"), changed_body).unwrap();
    let changed_file_hash = sha256_hex(changed_body);
    current["files"]["incodex-main.cjs"] = changed_file_hash.clone().into();

    let manifest_path = old_release.join("runtime-manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["files"]["incodex-main.cjs"] = changed_file_hash.into();
    let manifest_body = format!("{}\n", serde_json::to_string_pretty(&manifest).unwrap());
    fs::write(&manifest_path, manifest_body.as_bytes()).unwrap();
    let deployed_manifest = sha256_hex(manifest_body.as_bytes());

    let version = current["version"].as_str().unwrap();
    let new_release_name = format!("{version}-{deployed_manifest}");
    let new_release = runtime_root(home).join("releases").join(&new_release_name);
    fs::rename(&old_release, &new_release).unwrap();
    current["release"] = format!("releases/{new_release_name}").into();
    current["manifestSha256"] = deployed_manifest.clone().into();
    write_current(home, &current);

    (bundled_manifest, deployed_manifest)
}

fn make_same_version_legacy_content_drift(home: &Path) {
    let mut current = publish_embedded_runtime(home);
    let release = current["release"].as_str().unwrap();
    let release_dir = runtime_root(home).join(release);
    let changed_body = b"legacy pointer with different Runtime content\n";
    fs::write(release_dir.join("incodex-main.cjs"), changed_body).unwrap();
    current["files"]["incodex-main.cjs"] = sha256_hex(changed_body).into();
    current.as_object_mut().unwrap().remove("manifestSha256");
    current.as_object_mut().unwrap().remove("sourceCommit");
    write_current(home, &current);
}

fn make_legacy_version_drift_with_matching_files(home: &Path) {
    let mut current = publish_embedded_runtime(home);
    current["version"] = "0.3.1".into();
    current.as_object_mut().unwrap().remove("manifestSha256");
    current.as_object_mut().unwrap().remove("sourceCommit");
    write_current(home, &current);
}

fn make_legacy_pointer_with_forged_manifest_suffix(home: &Path) {
    let mut current = publish_embedded_runtime(home);
    let old_release = runtime_root(home).join(current["release"].as_str().unwrap());
    let version = current["version"].as_str().unwrap();
    let forged_name = format!("{version}-{}", "0".repeat(64));
    fs::rename(
        &old_release,
        runtime_root(home).join("releases").join(&forged_name),
    )
    .unwrap();
    current["release"] = format!("releases/{forged_name}").into();
    current.as_object_mut().unwrap().remove("manifestSha256");
    current.as_object_mut().unwrap().remove("sourceCommit");
    write_current(home, &current);
}

fn assert_runtime_drift(command: &str, home: &Path, app: &Path) {
    let app = app.to_str().unwrap();
    let (status, stdout, stderr) = run(&[command, "--json", "--app", app], home);
    assert_eq!(status, 0, "{command}: {stderr}");
    assert_eq!(stderr, "", "{command}");
    let report = parse_json(&stdout);
    let runtime = &report["externalRuntime"];
    assert_eq!(runtime["present"], true, "{command}");
    assert_eq!(runtime["ok"], true, "{command}");
    assert_eq!(runtime["state"], "stale", "{command}");
    assert_eq!(runtime["matchesBundled"], false, "{command}");
    assert!(
        report["checks"]["runtime"]["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| finding["code"] == "runtime.stale"),
        "{command}: {stdout}"
    );
}

#[test]
fn status_and_doctor_detect_same_version_manifest_drift() {
    let home = isolated_home();
    let app = diagnostic_app(&home);
    let (bundled_manifest, deployed_manifest) = make_same_version_canonical_drift(&home);
    assert_ne!(bundled_manifest, deployed_manifest);

    for command in ["status", "doctor"] {
        assert_runtime_drift(command, &home, &app);
        let (_, stdout, _) = run(&[command, "--json", "--app", app.to_str().unwrap()], &home);
        let runtime = &parse_json(&stdout)["externalRuntime"];
        assert_eq!(runtime["version"], runtime["bundledVersion"], "{command}");
        assert_eq!(
            runtime["bundledManifestSha256"], bundled_manifest,
            "{command}"
        );
        assert_eq!(runtime["manifestSha256"], deployed_manifest, "{command}");
    }
}

#[test]
fn status_and_doctor_detect_legacy_content_drift_without_using_version() {
    let home = isolated_home();
    let app = diagnostic_app(&home);
    make_same_version_legacy_content_drift(&home);

    for command in ["status", "doctor"] {
        assert_runtime_drift(command, &home, &app);
        let (_, stdout, _) = run(&[command, "--json", "--app", app.to_str().unwrap()], &home);
        let runtime = &parse_json(&stdout)["externalRuntime"];
        assert_eq!(runtime["version"], runtime["bundledVersion"], "{command}");
        assert!(runtime["manifestSha256"].is_null(), "{command}");
    }
}

#[test]
fn status_and_doctor_mark_0_3_1_stale_even_if_legacy_files_match() {
    let home = isolated_home();
    let app = diagnostic_app(&home);
    make_legacy_version_drift_with_matching_files(&home);

    for command in ["status", "doctor"] {
        assert_runtime_drift(command, &home, &app);
        let (_, stdout, _) = run(&[command, "--json", "--app", app.to_str().unwrap()], &home);
        let runtime = &parse_json(&stdout)["externalRuntime"];
        assert_eq!(runtime["version"], "0.3.1", "{command}");
        assert_eq!(
            runtime["bundledVersion"],
            env!("CARGO_PKG_VERSION"),
            "{command}"
        );
        assert_ne!(runtime["version"], runtime["bundledVersion"], "{command}");
    }
}

#[test]
fn matching_runtime_is_current_and_text_reports_runtime_state() {
    let home = isolated_home();
    let app = diagnostic_app(&home);
    let current = publish_embedded_runtime(&home);

    for command in ["status", "doctor"] {
        let (status, stdout, stderr) =
            run(&[command, "--json", "--app", app.to_str().unwrap()], &home);
        assert_eq!(status, 0, "{command}: {stderr}");
        let report = parse_json(&stdout);
        let runtime = &report["externalRuntime"];
        assert_eq!(runtime["state"], "current", "{command}");
        assert_eq!(runtime["matchesBundled"], true, "{command}");
        assert_eq!(
            runtime["bundledManifestSha256"], current["manifestSha256"],
            "{command}"
        );
        assert_eq!(
            runtime["manifestSha256"], current["manifestSha256"],
            "{command}"
        );

        let (status, stdout, stderr) = run(&[command, "--app", app.to_str().unwrap()], &home);
        assert_eq!(status, 0, "{command}: {stderr}");
        assert!(stdout.contains("Runtime state"), "{command}: {stdout}");
        assert!(stdout.contains("current"), "{command}: {stdout}");
    }
}

#[test]
fn legacy_pointer_never_invents_manifest_provenance_from_its_release_name() {
    let home = isolated_home();
    let app = diagnostic_app(&home);
    make_legacy_pointer_with_forged_manifest_suffix(&home);

    for command in ["status", "doctor"] {
        let (status, stdout, stderr) =
            run(&[command, "--json", "--app", app.to_str().unwrap()], &home);
        assert_eq!(status, 0, "{command}: {stderr}");
        let runtime = &parse_json(&stdout)["externalRuntime"];
        assert_eq!(runtime["state"], "current", "{command}");
        assert_eq!(runtime["matchesBundled"], true, "{command}");
        assert!(runtime["manifestSha256"].is_null(), "{command}: {stdout}");
    }
}

#[test]
fn stale_runtime_text_preserves_exit_code_and_prints_the_repair_command() {
    let home = isolated_home();
    let app = diagnostic_app(&home);
    make_same_version_canonical_drift(&home);

    for command in ["status", "doctor"] {
        let (status, stdout, stderr) = run(&[command, "--app", app.to_str().unwrap()], &home);
        assert_eq!(status, 0, "{command}: {stderr}");
        assert_eq!(stderr, "", "{command}");
        assert!(stdout.contains("CLI Runtime"), "{command}: {stdout}");
        assert!(stdout.contains("Runtime state"), "{command}: {stdout}");
        assert!(stdout.contains("stale"), "{command}: {stdout}");
        assert!(stdout.contains("incodex runtime"), "{command}: {stdout}");
    }
}

#[test]
fn missing_runtime_text_still_reports_the_bundled_runtime_and_missing_state() {
    let home = isolated_home();
    let app = home.join("Missing.app");

    for command in ["status", "doctor"] {
        let (status, stdout, stderr) = run(&[command, "--app", app.to_str().unwrap()], &home);
        assert_eq!(status, 0, "{command}: {stderr}");
        assert_eq!(stderr, "", "{command}");
        assert!(stdout.contains("CLI Runtime"), "{command}: {stdout}");
        assert!(
            stdout.contains(env!("CARGO_PKG_VERSION")),
            "{command}: {stdout}"
        );
        assert!(stdout.contains("Runtime state"), "{command}: {stdout}");
        assert!(stdout.contains("missing"), "{command}: {stdout}");
        if command == "doctor" {
            assert!(stdout.contains("Deployed manifest"), "{stdout}");
            assert!(stdout.contains("not published"), "{stdout}");
        }
    }
}
