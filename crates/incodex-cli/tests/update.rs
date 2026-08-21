use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

mod support;
#[path = "support/update_menu.rs"]
mod update_menu;

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
fn update_pty_harness_reaps_after_child_closes_before_exit() {
    let home = scratch("pty-close-before-exit");
    let fake_update = home.join("fake-update");
    write_executable(
        &fake_update,
        "#!/bin/sh\nexec 0<&- 1>&- 2>&-\nsleep 2\n",
    );

    let result = support::tty::run_with_timeout_env(
        fake_update.to_str().unwrap(),
        &["update"],
        &[],
        &home,
        "update prompt that never arrives",
        "",
        Duration::from_millis(100),
        &[("PATH", "/usr/bin:/bin")],
    );

    assert_eq!(
        result.status, 124,
        "update PTY timeout was not reported: {result:?}"
    );
    assert!(
        result.stderr.contains("timed out"),
        "update PTY timeout lacked diagnostics: {result:?}"
    );
}

fn run_tty(program: &std::path::Path, home: &std::path::Path, path: &str) -> (i32, String) {
    let result = support::tty::run_with_timeout_env(
        program.to_str().unwrap(),
        &["update"],
        &[],
        home,
        "__INCODEX_UPDATE_PROMPT_NEVER__",
        "",
        Duration::from_secs(12),
        &[("PATH", path)],
    );
    (result.status, result.stdout)
}

#[test]
fn update_does_not_retry_a_permanent_http_failure() {
    let home = scratch("download-failure");
    let prefix = home.join("prefix");
    let bin = prefix.join("bin");
    let fake_bin = home.join("fake-bin");
    let attempts = home.join("attempts");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&fake_bin).unwrap();

    let curl = fake_bin.join("curl");
    write_executable(
        &curl,
        "#!/bin/sh\ncount=$(cat \"$ATTEMPTS\" 2>/dev/null || printf '0')\nprintf '%s\\n' \"$((count + 1))\" > \"$ATTEMPTS\"\nprintf 'INCODEX_HTTP_STATUS:404'\nexit 22\n",
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
        .env("ATTEMPTS", &attempts)
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
    assert_eq!(fs::read_to_string(attempts).unwrap().trim(), "1");
}

#[test]
fn update_animates_network_stages_and_clears_them_before_success() {
    let home = scratch("tty-progress");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let (prefix, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        r#"#!/bin/sh
url=""
for arg in "$@"; do url="$arg"; done
case "$url" in
  https://api.github.com/repos/daftAI2026/incodex/releases/latest)
    printf '%s' '{"tag_name":"v9.9.9"}'
    ;;
  https://raw.githubusercontent.com/daftAI2026/incodex/v9.9.9/install.sh)
    cat <<'INSTALLER'
#!/bin/sh
printf '%s\n' 'STABLE-INSTALLER-OUT'
printf '%s\n' 'STABLE-INSTALLER-ERR' >&2
sleep 0.2
cat > "$INCODEX_PREFIX/bin/incodex.next" <<'CLI'
#!/bin/sh
printf '%s\n' 'Incodex version 9.9.9'
CLI
chmod +x "$INCODEX_PREFIX/bin/incodex.next"
mv "$INCODEX_PREFIX/bin/incodex.next" "$INCODEX_PREFIX/bin/incodex"
INSTALLER
    ;;
  *) exit 88 ;;
esac
printf '%s' 'INCODEX_HTTP_STATUS:200'
"#,
    );

    let path = format!("{}:/usr/bin:/bin", fake_bin.display());
    let (status, output) = run_tty(&installed, &home, &path);
    assert_eq!(status, 0, "{output:?}");
    for stage in [
        "Checking for updates",
        "Downloading stable installer",
        "Installing v9.9.9",
    ] {
        assert!(
            ["|", "/", "-", "\\"]
                .iter()
                .any(|frame| output.contains(&format!("  {frame} {stage}"))),
            "update stage {stage:?} did not animate: {output:?}"
        );
    }
    assert!(
        output.contains("\r\u{1b}[2K"),
        "update progress did not clear: {output:?}"
    );
    assert!(output.contains("Verified Incodex 9.9.9"), "{output:?}");
    assert!(!output.contains("STABLE-INSTALLER-OUT"), "{output:?}");
    assert!(!output.contains("STABLE-INSTALLER-ERR"), "{output:?}");
    assert_eq!(prefix.join("bin/incodex"), installed);
}

#[test]
fn update_keeps_animating_while_the_compatibility_installer_runs() {
    let home = scratch("tty-compatibility-progress");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let (_, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        r#"#!/bin/sh
url=""
for arg in "$@"; do url="$arg"; done
case "$url" in
  https://api.github.com/repos/daftAI2026/incodex/releases/latest)
    printf '%s' '{"tag_name":"v9.9.9"}'
    ;;
  https://raw.githubusercontent.com/daftAI2026/incodex/v9.9.9/install.sh)
    printf '%s\n' '#!/bin/sh' 'sleep 0.2' 'printf "%s\\n" "TAGGED-INSTALLER-ERROR" >&2' 'exit 1'
    ;;
  https://raw.githubusercontent.com/daftAI2026/incodex/main/install.sh)
    cat <<'INSTALLER'
#!/bin/sh
printf '%s\n' 'COMPAT-INSTALLER-OUT'
printf '%s\n' 'COMPAT-INSTALLER-ERR' >&2
sleep 0.2
cat > "$INCODEX_PREFIX/bin/incodex.next" <<'CLI'
#!/bin/sh
printf '%s\n' 'Incodex version 9.9.9'
CLI
chmod +x "$INCODEX_PREFIX/bin/incodex.next"
mv "$INCODEX_PREFIX/bin/incodex.next" "$INCODEX_PREFIX/bin/incodex"
INSTALLER
    ;;
  *) exit 88 ;;
esac
printf '%s' 'INCODEX_HTTP_STATUS:200'
"#,
    );

    let path = format!("{}:/usr/bin:/bin", fake_bin.display());
    let (status, output) = run_tty(&installed, &home, &path);
    assert_eq!(status, 0, "{output:?}");
    for stage in ["Installing v9.9.9", "Repairing v9.9.9"] {
        assert!(
            ["|", "/", "-", "\\"]
                .iter()
                .any(|frame| output.contains(&format!("  {frame} {stage}"))),
            "update stage {stage:?} did not animate: {output:?}"
        );
    }
    assert!(
        output.contains(
            "Stable installer did not complete: update failed: installer exited with exit status: 1: TAGGED-INSTALLER-ERROR"
        ),
        "{output:?}"
    );
    assert_eq!(output.matches("TAGGED-INSTALLER-ERROR").count(), 1, "{output:?}");
    assert!(!output.contains("COMPAT-INSTALLER-OUT"), "{output:?}");
    assert!(!output.contains("COMPAT-INSTALLER-ERR"), "{output:?}");
    assert!(output.contains("Verified Incodex 9.9.9"), "{output:?}");
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
    let expected_version_log = home.join("expected-version.log");
    let download_dir_log = home.join("download-dir.log");
    let arch_log = home.join("arch.log");
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
printf '%s\n' "$INCODEX_EXPECTED_VERSION" > "$EXPECTED_VERSION_LOG"
printf '%s\n' "${INCODEX_DOWNLOAD_DIR-unset}" > "$DOWNLOAD_DIR_LOG"
printf '%s\n' "${INCODEX_ARCH-unset}" > "$ARCH_LOG"
cat > "$INCODEX_PREFIX/bin/incodex.next" <<'CLI'
#!/bin/sh
printf '%s\n' 'Incodex version 9.9.9'
CLI
chmod 755 "$INCODEX_PREFIX/bin/incodex.next"
mv -f "$INCODEX_PREFIX/bin/incodex.next" "$INCODEX_PREFIX/bin/incodex"
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
        .env("EXPECTED_VERSION_LOG", &expected_version_log)
        .env("DOWNLOAD_DIR_LOG", &download_dir_log)
        .env("ARCH_LOG", &arch_log)
        .env("INCODEX_PREFIX", home.join("attacker-prefix"))
        .env("INCODEX_DOWNLOAD_BASE", "https://attacker.invalid/release")
        .env("INCODEX_DOWNLOAD_DIR", home.join("attacker-release"))
        .env("INCODEX_ARCH", "x86_64")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(prefix_log).unwrap().trim(),
        prefix.to_string_lossy()
    );
    assert_eq!(
        fs::read_to_string(download_base_log).unwrap().trim(),
        "https://github.com/daftAI2026/incodex/releases/download/v9.9.9"
    );
    assert_eq!(
        fs::read_to_string(expected_version_log).unwrap().trim(),
        "9.9.9"
    );
    assert_eq!(
        fs::read_to_string(download_dir_log).unwrap().trim(),
        "unset"
    );
    assert_eq!(fs::read_to_string(arch_log).unwrap().trim(), "unset");
    let urls = fs::read_to_string(curl_log).unwrap();
    assert!(urls.contains("raw.githubusercontent.com/daftAI2026/incodex/v9.9.9/install.sh"));
    assert!(!urls.contains("raw.githubusercontent.com/daftAI2026/incodex/main/install.sh"));
}

#[test]
fn update_retries_transient_release_lookup_and_announces_each_stage() {
    let home = scratch("retry-progress");
    let fake_bin = home.join("fake-bin");
    let attempts = home.join("attempts");
    let installer_attempts = home.join("installer-attempts");
    fs::create_dir_all(&fake_bin).unwrap();
    let (_, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        r#"#!/bin/sh
url=""
for arg in "$@"; do url="$arg"; done
case "$url" in
  https://api.github.com/repos/daftAI2026/incodex/releases/latest)
    count=$(cat "$ATTEMPTS" 2>/dev/null || printf '0')
    count=$((count + 1))
    printf '%s\n' "$count" > "$ATTEMPTS"
    if [ "$count" -lt 3 ]; then printf 'INCODEX_HTTP_STATUS:503'; exit 22; fi
    printf '%s\n' '{"tag_name":"v9.9.9"}'
    ;;
  https://raw.githubusercontent.com/daftAI2026/incodex/v9.9.9/install.sh)
    count=$(cat "$INSTALLER_ATTEMPTS" 2>/dev/null || printf '0')
    count=$((count + 1))
    printf '%s\n' "$count" > "$INSTALLER_ATTEMPTS"
    if [ "$count" -lt 3 ]; then printf 'INCODEX_HTTP_STATUS:503'; exit 22; fi
    cat <<'INSTALLER'
#!/bin/sh
cat > "$INCODEX_PREFIX/bin/incodex.next" <<'CLI'
#!/bin/sh
printf '%s\n' 'Incodex version 9.9.9'
CLI
chmod 755 "$INCODEX_PREFIX/bin/incodex.next"
mv -f "$INCODEX_PREFIX/bin/incodex.next" "$INCODEX_PREFIX/bin/incodex"
INSTALLER
    ;;
  *) exit 88 ;;
esac
"#,
    );

    let output = Command::new(&installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("ATTEMPTS", &attempts)
        .env("INSTALLER_ATTEMPTS", &installer_attempts)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(attempts).unwrap().trim(), "3");
    assert_eq!(fs::read_to_string(installer_attempts).unwrap().trim(), "3");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for stage in [
        "Checking for updates",
        "Downloading stable installer",
        "Installing v9.9.9",
        "Verified Incodex 9.9.9",
    ] {
        assert!(stdout.contains(stage), "missing stage {stage:?}: {stdout}");
    }
}

#[test]
fn update_rejects_false_success_when_the_installed_binary_did_not_change() {
    let home = scratch("false-success");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let (_, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        r#"#!/bin/sh
url=""
for arg in "$@"; do url="$arg"; done
case "$url" in
  https://api.github.com/repos/daftAI2026/incodex/releases/latest)
    printf '%s\n' '{"tag_name":"v9.9.9"}' ;;
  https://raw.githubusercontent.com/daftAI2026/incodex/v9.9.9/install.sh)
    printf '%s\n' '#!/bin/sh' 'exit 0' ;;
  https://raw.githubusercontent.com/daftAI2026/incodex/main/install.sh)
    printf '%s\n' '#!/bin/sh' 'exit 0' ;;
  *) exit 88 ;;
esac
"#,
    );

    let output = Command::new(&installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("installed CLI did not report 9.9.9"),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn update_uses_main_only_to_heal_a_failed_tagged_installer() {
    let home = scratch("self-heal");
    let fake_bin = home.join("fake-bin");
    let curl_log = home.join("curl.log");
    let download_base_log = home.join("download-base.log");
    fs::create_dir_all(&fake_bin).unwrap();
    let (_, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$CURL_LOG"
url=""
for arg in "$@"; do url="$arg"; done
case "$url" in
  https://api.github.com/repos/daftAI2026/incodex/releases/latest)
    printf '%s\n' '{"tag_name":"v9.9.9"}' ;;
  https://raw.githubusercontent.com/daftAI2026/incodex/v9.9.9/install.sh)
    printf '%s\n' '#!/bin/sh' 'exit 42' ;;
  https://raw.githubusercontent.com/daftAI2026/incodex/main/install.sh)
    cat <<'INSTALLER'
#!/bin/sh
printf '%s\n' "$INCODEX_DOWNLOAD_BASE" > "$DOWNLOAD_BASE_LOG"
cat > "$INCODEX_PREFIX/bin/incodex.next" <<'CLI'
#!/bin/sh
printf '%s\n' 'Incodex version 9.9.9'
CLI
chmod 755 "$INCODEX_PREFIX/bin/incodex.next"
mv -f "$INCODEX_PREFIX/bin/incodex.next" "$INCODEX_PREFIX/bin/incodex"
INSTALLER
    ;;
  *) exit 88 ;;
esac
"#,
    );

    let output = Command::new(&installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("CURL_LOG", &curl_log)
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
    let urls = fs::read_to_string(curl_log).unwrap();
    let tag_pos = urls.find("/v9.9.9/install.sh").unwrap();
    let heal_pos = urls.find("/main/install.sh").unwrap();
    assert!(tag_pos < heal_pos, "main was not a fallback: {urls}");
    assert_eq!(
        fs::read_to_string(download_base_log).unwrap().trim(),
        "https://github.com/daftAI2026/incodex/releases/download/v9.9.9"
    );
}

#[test]
fn overlapping_updates_are_refused_by_the_target_lock() {
    let home = scratch("lock");
    let fake_bin = home.join("fake-bin");
    let installer_started = home.join("installer-started");
    let installer_gate = home.join("installer-gate");
    let installer_release = home.join("installer-release");
    fs::create_dir_all(&fake_bin).unwrap();
    let (_, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        r#"#!/bin/sh
url=""
for arg in "$@"; do url="$arg"; done
case "$url" in
  https://api.github.com/repos/daftAI2026/incodex/releases/latest)
    printf '%s\n' '{"tag_name":"v9.9.9"}' ;;
  https://raw.githubusercontent.com/daftAI2026/incodex/v9.9.9/install.sh)
    cat <<'INSTALLER'
#!/bin/sh
if mkdir "$INSTALLER_GATE" 2>/dev/null; then
  : > "$INSTALLER_STARTED"
  while [ ! -f "$INSTALLER_RELEASE" ]; do sleep 0.05; done
  exit 1
fi
exit 0
INSTALLER
    ;;
  https://raw.githubusercontent.com/daftAI2026/incodex/main/install.sh)
    printf '%s\n' '#!/bin/sh' 'exit 1' ;;
  *) exit 88 ;;
esac
"#,
    );

    let path = format!("{}:/usr/bin:/bin", fake_bin.display());
    let mut first = Command::new(&installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", &path)
        .env("INSTALLER_STARTED", &installer_started)
        .env("INSTALLER_GATE", &installer_gate)
        .env("INSTALLER_RELEASE", &installer_release)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !installer_started.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        installer_started.exists(),
        "first updater never entered installer"
    );

    let started = Instant::now();
    let second = Command::new(&installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", &path)
        .env("INSTALLER_STARTED", &installer_started)
        .env("INSTALLER_GATE", &installer_gate)
        .env("INSTALLER_RELEASE", &installer_release)
        .output()
        .unwrap();
    let elapsed = started.elapsed();
    fs::write(&installer_release, "release\n").unwrap();
    let _ = first.wait();

    assert_eq!(second.status.code(), Some(1));
    assert!(
        elapsed < Duration::from_secs(1),
        "second updater waited {elapsed:?}"
    );
    assert!(
        String::from_utf8_lossy(&second.stderr).contains("another update is already running"),
        "stderr={}",
        String::from_utf8_lossy(&second.stderr)
    );
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
