#![cfg(target_os = "windows")]

use std::path::PathBuf;
use std::{fs, path::Path};

use incodex_cli::windows_update::{
    acquire_windows_install_lock, clear_windows_runtime_pending, expected_release_sha256,
    parse_windows_main_commit, parse_windows_stable_release, read_windows_runtime_pending,
    validate_managed_install_identity, validate_windows_download_size, validate_windows_user_root,
    windows_release_asset, write_windows_runtime_pending, WindowsStandaloneLayout,
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

#[test]
fn managed_channel_must_point_at_the_running_versioned_binary() {
    let root = std::env::temp_dir().join(format!(
        "incodex-windows-update-identity-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    let package_root = root.join("packages/standalone");
    let release = package_root.join("releases/9.9.9");
    fs::create_dir_all(&release).expect("create managed release");
    let managed = release.join("incodex.exe");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &managed).expect("copy managed CLI");
    fs::write(package_root.join("current"), "9.9.9\n").expect("write generation marker");

    assert_eq!(
        validate_managed_install_identity(&package_root, &managed)
            .expect("matching managed identity"),
        root
    );

    let external = root.join("outside.exe");
    fs::copy(Path::new(env!("CARGO_BIN_EXE_incodex")), &external).expect("copy external CLI");
    assert!(validate_managed_install_identity(&package_root, &external)
        .expect_err("spoofed package root must fail closed")
        .contains("running Windows CLI"));

    fs::remove_dir_all(root).expect("remove identity fixture");
}

#[test]
fn compatibility_installer_is_pinned_to_an_immutable_main_commit() {
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let snapshot = parse_windows_main_commit(format!(r#"{{"sha":"{sha}"}}"#).as_bytes())
        .expect("canonical main commit");
    assert_eq!(snapshot.commit(), sha);
    assert_eq!(
        snapshot.installer_url(),
        format!("https://raw.githubusercontent.com/daftAI2026/incodex/{sha}/install.ps1")
    );

    for invalid in ["main", "abc", "g123456789abcdef0123456789abcdef01234567"] {
        let metadata = format!(r#"{{"sha":"{invalid}"}}"#);
        assert!(parse_windows_main_commit(metadata.as_bytes()).is_err());
    }
}

#[test]
fn runtime_handoff_is_durable_until_the_expected_cli_completes_it() {
    let root = std::env::temp_dir().join(format!(
        "incodex-windows-runtime-pending-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));

    write_windows_runtime_pending(&root, "9.9.9").expect("write pending handoff");
    assert_eq!(
        read_windows_runtime_pending(&root).expect("read pending handoff"),
        Some("9.9.9".to_string())
    );
    clear_windows_runtime_pending(&root).expect("clear pending handoff");
    assert_eq!(
        read_windows_runtime_pending(&root).expect("read cleared handoff"),
        None
    );
    assert!(write_windows_runtime_pending(&root, "v9.9.9").is_err());

    fs::remove_dir_all(root).expect("remove pending fixture");
}

#[test]
fn post_install_verification_uses_the_installer_generation_lock() {
    let root = std::env::temp_dir().join(format!(
        "incodex-windows-update-lock-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ));
    let root = incodex_core::windows_session::ensure_private_windows_dir(&root)
        .expect("create private lock fixture");

    let first = acquire_windows_install_lock(&root).expect("acquire first lock");
    assert!(acquire_windows_install_lock(&root)
        .expect_err("second update must not cross the active generation")
        .contains("another Incodex install or update"));
    drop(first);
    acquire_windows_install_lock(&root).expect("lock is reusable after completion");

    fs::remove_dir_all(root).expect("remove lock fixture");
}

#[test]
fn managed_updates_stay_under_the_current_token_profile() {
    let profile = Path::new(r"C:\Users\Kid");
    validate_windows_user_root(Path::new(r"C:\Users\Kid\.incodex"), profile)
        .expect("default per-user installation");
    assert!(validate_windows_user_root(Path::new(r"D:\shared\incodex"), profile).is_err());
    assert!(
        validate_windows_user_root(Path::new(r"C:\Users\Kid\.incodex\nested"), profile).is_err()
    );
}

#[test]
fn update_metadata_and_scripts_have_explicit_size_bounds() {
    validate_windows_download_size("release metadata", 1, 256 * 1024).expect("bounded metadata");
    assert!(validate_windows_download_size("release metadata", 0, 256 * 1024).is_err());
    assert!(
        validate_windows_download_size("stable installer", 1024 * 1024 + 1, 1024 * 1024).is_err()
    );
}
