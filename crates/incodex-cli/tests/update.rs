use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch(label: &str) -> PathBuf {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "incodex-update-{label}-{}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn write_executable(path: &std::path::Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn installed_cli(home: &std::path::Path) -> (PathBuf, PathBuf) {
    let prefix = home.join("prefix");
    let bin = prefix.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let installed = bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    (prefix, installed)
}

#[test]
fn update_fails_when_install_script_download_fails() {
    let home = scratch("download-failure");
    let prefix = home.join("prefix");
    let bin = prefix.join("bin");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&fake_bin).unwrap();

    let curl = fake_bin.join("curl");
    write_executable(
        &curl,
        "#!/bin/sh\nprintf '%s\\n' 'simulated download failure' >&2\nexit 22\n",
    );

    let bash_profile = home.join(".bash_profile");
    fs::write(
        &bash_profile,
        format!("export PATH='{}:/usr/bin:/bin'\n", fake_bin.display()),
    )
    .unwrap();

    let installed = bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    let output = Command::new(&installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "update unexpectedly succeeded:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("update failed"));
}

#[test]
fn update_stops_when_the_stable_release_is_already_installed() {
    let home = scratch("already-current");
    let fake_bin = home.join("fake-bin");
    let curl_log = home.join("curl.log");
    fs::create_dir_all(&fake_bin).unwrap();
    let (_, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$CURL_LOG\"\ncase \"$*\" in\n  *api.github.com/repos/daftAI2026/incodex/releases/latest*) printf '{\"tag_name\":\"v%s\"}\\n' \"$CURRENT_VERSION\" ;;\n  *) printf '%s\\n' 'unexpected installer download' >&2; exit 88 ;;\nesac\n",
    );

    let output = Command::new(&installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("CURL_LOG", &curl_log)
        .env("CURRENT_VERSION", env!("CARGO_PKG_VERSION"))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).contains(&format!(
        "Already on latest version, {}",
        env!("CARGO_PKG_VERSION")
    )));
    let urls = fs::read_to_string(curl_log).unwrap();
    assert!(urls.contains("api.github.com/repos/daftAI2026/incodex/releases/latest"));
    assert!(!urls.contains("raw.githubusercontent.com"));
}

#[test]
fn update_pins_the_installer_and_assets_to_the_resolved_release() {
    let home = scratch("pinned-release");
    let fake_bin = home.join("fake-bin");
    let curl_log = home.join("curl.log");
    let prefix_log = home.join("prefix.log");
    let download_base_log = home.join("download-base.log");
    fs::create_dir_all(&fake_bin).unwrap();
    let (prefix, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$CURL_LOG"
url=""
for arg in "$@"; do url="$arg"; done
case "$url" in
  https://api.github.com/repos/daftAI2026/incodex/releases/latest)
    printf '%s\n' '{"tag_name":"v9.9.9"}'
    ;;
  https://raw.githubusercontent.com/daftAI2026/incodex/v9.9.9/install.sh)
    cat <<'INSTALLER'
#!/bin/sh
printf '%s\n' "$INCODEX_PREFIX" > "$PREFIX_LOG"
printf '%s\n' "$INCODEX_DOWNLOAD_BASE" > "$DOWNLOAD_BASE_LOG"
INSTALLER
    ;;
  *)
    printf 'unexpected URL: %s\n' "$url" >&2
    exit 88
    ;;
esac
"#,
    );

    let output = Command::new(&installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("CURL_LOG", &curl_log)
        .env("PREFIX_LOG", &prefix_log)
        .env("DOWNLOAD_BASE_LOG", &download_base_log)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(prefix_log).unwrap().trim(), prefix.to_string_lossy());
    assert_eq!(
        fs::read_to_string(download_base_log).unwrap().trim(),
        "https://github.com/daftAI2026/incodex/releases/download/v9.9.9"
    );
    let urls = fs::read_to_string(curl_log).unwrap();
    assert!(urls.contains("raw.githubusercontent.com/daftAI2026/incodex/v9.9.9/install.sh"));
    assert!(!urls.contains("raw.githubusercontent.com/daftAI2026/incodex/main/install.sh"));
}

#[test]
fn update_rejects_a_non_release_tag_before_downloading_an_installer() {
    let home = scratch("invalid-tag");
    let fake_bin = home.join("fake-bin");
    let curl_log = home.join("curl.log");
    fs::create_dir_all(&fake_bin).unwrap();
    let (_, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$CURL_LOG\"\nprintf '%s\\n' '{\"tag_name\":\"main\"}'\n",
    );

    let output = Command::new(&installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("CURL_LOG", &curl_log)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid latest release tag"));
    let urls = fs::read_to_string(curl_log).unwrap();
    assert_eq!(urls.lines().count(), 1);
    assert!(!urls.contains("raw.githubusercontent.com"));
}
