#![cfg(target_os = "windows")]

use std::path::PathBuf;

use incodex_cli::windows_update::{
    expected_release_sha256, parse_windows_stable_release, windows_release_asset,
    WindowsStandaloneLayout,
};

#[test]
fn selects_only_the_published_windows_architecture() {
    assert_eq!(
        windows_release_asset("x86_64").expect("x64 release asset"),
        "incodex-windows-x64.exe"
    );
    assert!(windows_release_asset("aarch64")
        .expect_err("ARM64 is not published yet")
        .contains("unsupported Windows architecture"));
}

#[test]
fn checksum_manifest_requires_one_exact_asset_entry() {
    let expected = "1f".repeat(32);
    let manifest = format!(
        "{}  incodex-darwin-arm64\n{expected}  incodex-windows-x64.exe\n",
        "00".repeat(32)
    );
    assert_eq!(
        expected_release_sha256(&manifest, "incodex-windows-x64.exe")
            .expect("exact checksum entry"),
        expected
    );

    let duplicate = format!("{manifest}{expected}  incodex-windows-x64.exe\n");
    assert!(
        expected_release_sha256(&duplicate, "incodex-windows-x64.exe")
            .expect_err("duplicate checksum must fail closed")
            .contains("exactly one")
    );
    assert!(
        expected_release_sha256(&manifest, "incodex-windows-arm64.exe")
            .expect_err("missing checksum must fail closed")
            .contains("exactly one")
    );
}

#[test]
fn standalone_layout_keeps_running_binaries_in_versioned_releases() {
    let root = PathBuf::from(r"C:\Users\Kid\.incodex");
    let layout = WindowsStandaloneLayout::new(&root);

    assert_eq!(layout.bin_dir(), root.join("bin"));
    assert_eq!(
        layout.package_root(),
        root.join("packages").join("standalone")
    );
    assert_eq!(
        layout
            .release_executable("0.6.0")
            .expect("stable release path"),
        root.join("packages")
            .join("standalone")
            .join("releases")
            .join("0.6.0")
            .join("incodex.exe")
    );
    assert_eq!(
        layout.primary_launcher(),
        root.join("bin").join("incodex.cmd")
    );
    assert_eq!(layout.alias_launcher(), root.join("bin").join("inc.cmd"));

    for invalid in ["v0.6.0", "0.6", "0.6.0-beta", "..", r"0.6.0\evil"] {
        assert!(layout.release_executable(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn update_plan_pins_the_installer_and_assets_to_one_stable_release() {
    let release =
        parse_windows_stable_release(br#"{"tag_name":"v9.9.9"}"#).expect("stable GitHub release");

    assert_eq!(release.tag(), "v9.9.9");
    assert_eq!(release.version(), "9.9.9");
    assert_eq!(
        release.installer_url(),
        "https://raw.githubusercontent.com/daftAI2026/incodex/v9.9.9/install.ps1"
    );
    assert_eq!(
        release.download_base(),
        "https://github.com/daftAI2026/incodex/releases/download/v9.9.9"
    );
}

#[test]
fn update_plan_rejects_noncanonical_or_prerelease_tags() {
    for tag in ["9.9.9", "v09.9.9", "v9.9.9-beta.1", "v9.9", "v9.9.9/evil"] {
        let metadata = format!(r#"{{"tag_name":"{tag}"}}"#);
        assert!(
            parse_windows_stable_release(metadata.as_bytes()).is_err(),
            "accepted {tag}"
        );
    }
}
