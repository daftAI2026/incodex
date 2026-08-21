use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

mod support;

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
    write_executable(&fake_update, "#!/bin/sh\nexec 0<&- 1>&- 2>&-\nsleep 30\n");

    let started = Instant::now();
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
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "update PTY harness waited for a closed child: {:?}",
        started.elapsed()
    );
}

fn run_tty(program: &std::path::Path, home: &std::path::Path, path: &str) -> (i32, String) {
    let _pty_gate = support::tty::acquire();
    let script = r#"
import os, pty, select, sys, time
program, home, path = sys.argv[1:]
env = os.environ.copy()
env["HOME"] = home
env["PATH"] = path
env["TERM"] = "xterm-256color"
env["NO_COLOR"] = "1"
pid, fd = pty.fork()
if pid == 0:
    os.execvpe(program, [program, "update"], env)
buf = bytearray()
deadline = time.time() + 12
while time.time() < deadline:
    ready, _, _ = select.select([fd], [], [], 0.1)
    if ready:
        try:
            chunk = os.read(fd, 8192)
        except OSError:
            _, status = os.waitpid(pid, 0)
            code = os.waitstatus_to_exitcode(status)
            sys.stdout.buffer.write(("STATUS %d\n" % code).encode())
            sys.stdout.buffer.write(bytes(buf))
            raise SystemExit(0)
        if not chunk:
            _, status = os.waitpid(pid, 0)
            code = os.waitstatus_to_exitcode(status)
            sys.stdout.buffer.write(("STATUS %d\n" % code).encode())
            sys.stdout.buffer.write(bytes(buf))
            raise SystemExit(0)
        buf.extend(chunk)
    done, status = os.waitpid(pid, os.WNOHANG)
    if done == pid:
        code = os.waitstatus_to_exitcode(status)
        sys.stdout.buffer.write(("STATUS %d\n" % code).encode())
        sys.stdout.buffer.write(bytes(buf))
        raise SystemExit(0)
os.kill(pid, 9)
os.waitpid(pid, 0)
sys.stdout.buffer.write(b"STATUS 124\n")
sys.stdout.buffer.write(bytes(buf))
"#;
    let output = Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(program)
        .arg(home)
        .arg(path)
        .output()
        .unwrap();
    let raw = String::from_utf8_lossy(&output.stdout).into_owned();
    let (status, output) = raw.split_once('\n').unwrap_or((&raw, ""));
    (
        status
            .strip_prefix("STATUS ")
            .and_then(|value| value.parse().ok())
            .unwrap_or(1),
        output.to_string(),
    )
}

fn open_menu_long_enough_for_background_refresh(
    home: &std::path::Path,
    installed: &std::path::Path,
    path: &str,
    keys: &str,
) {
    let _pty_gate = support::tty::acquire();
    let script = r#"
import os, pty, select, sys, time
program, home, path, keys = sys.argv[1:]
env = os.environ.copy()
env["HOME"] = home
env["PATH"] = path
env["TERM"] = "xterm-256color"
env["NO_COLOR"] = "1"
pid, fd = pty.fork()
if pid == 0:
    os.execvpe(program, [program], env)
deadline = time.time() + 5
seen = False
while time.time() < deadline:
    ready, _, _ = select.select([fd], [], [], 0.1)
    if ready:
        try:
            chunk = os.read(fd, 8192)
        except OSError:
            break
        if b"6. Quit" in chunk:
            seen = True
            break
if not seen:
    os.kill(pid, 9)
    os.waitpid(pid, 0)
    raise SystemExit(2)
cache = os.path.join(home, ".incodex", "cache", "update_message")
refresh_deadline = time.time() + 3
while time.time() < refresh_deadline:
    try:
        if open(cache, "rb").read().strip():
            break
    except OSError:
        pass
    time.sleep(0.05)
time.sleep(0.2)
for key in keys.encode("ascii"):
    try:
        os.write(fd, bytes([key]))
    except OSError:
        break
    time.sleep(0.2)
exit_deadline = time.time() + 3
while time.time() < exit_deadline:
    done, status = os.waitpid(pid, os.WNOHANG)
    if done == pid:
        raise SystemExit(os.waitstatus_to_exitcode(status))
    ready, _, _ = select.select([fd], [], [], 0.05)
    if ready:
        try:
            os.read(fd, 8192)
        except OSError:
            pass
os.kill(pid, 9)
os.waitpid(pid, 0)
raise SystemExit(3)
"#;
    let status = Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(installed)
        .arg(home)
        .arg(path)
        .arg(keys)
        .status()
        .unwrap();
    assert!(status.success(), "PTY menu harness failed: {status}");
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
fn native_script_menu_refreshes_the_stable_update_notice_cache() {
    let home = scratch("menu-refresh");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let (_, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        "#!/bin/sh\nprintf '%s\\n' '{\"tag_name\":\"v9.9.9\"}'\n",
    );
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());

    open_menu_long_enough_for_background_refresh(&home, &installed, &path, "q");

    let cache = home.join(".incodex/cache/update_message");
    assert_eq!(
        fs::read_to_string(cache).unwrap().trim(),
        "Update 9.9.9 available, run incodex update"
    );
}

#[test]
fn native_homebrew_menu_waits_for_the_formula_and_names_brew_upgrade() {
    let home = scratch("menu-homebrew-refresh");
    let fake_bin = home.join("fake-bin");
    let cellar_bin = home.join("Cellar/incodex/0.3.1/bin");
    fs::create_dir_all(&fake_bin).unwrap();
    fs::create_dir_all(&cellar_bin).unwrap();
    let installed = cellar_bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    write_executable(
        &fake_bin.join("curl"),
        "#!/bin/sh\nprintf '%s\\n' '{\"tag_name\":\"v9.9.9\"}'\n",
    );
    write_executable(
        &fake_bin.join("brew"),
        "#!/bin/sh\nprintf '%s\\n' '{\"formulae\":[{\"versions\":{\"stable\":\"9.9.9\"},\"installed\":[{\"version\":\"0.3.1\"}]}]}'\n",
    );
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());

    open_menu_long_enough_for_background_refresh(&home, &installed, &path, "q");

    let cache = home.join(".incodex/cache/update_message");
    assert_eq!(
        fs::read_to_string(cache).unwrap().trim(),
        "Update 9.9.9 available, run brew upgrade incodex"
    );
}

#[test]
fn native_homebrew_menu_does_not_expose_the_self_update_shortcut() {
    let home = scratch("menu-homebrew-no-self-update");
    let fake_bin = home.join("fake-bin");
    let cellar_bin = home.join(format!("Cellar/incodex/{}/bin", env!("CARGO_PKG_VERSION")));
    fs::create_dir_all(&fake_bin).unwrap();
    fs::create_dir_all(&cellar_bin).unwrap();
    let installed = cellar_bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    write_executable(&fake_bin.join("curl"), "#!/bin/sh\nexit 22\n");
    let cache = home.join(".incodex/cache/update_message");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    fs::write(cache, "Update 9.9.9 available, run brew upgrade incodex\n").unwrap();
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());

    open_menu_long_enough_for_background_refresh(&home, &installed, &path, "uq");
}

#[test]
fn native_homebrew_menu_rejects_a_current_script_update_notice() {
    let home = scratch("menu-homebrew-stale-script-notice");
    let fake_bin = home.join("fake-bin");
    let cellar_bin = home.join(format!("Cellar/incodex/{}/bin", env!("CARGO_PKG_VERSION")));
    fs::create_dir_all(&fake_bin).unwrap();
    fs::create_dir_all(&cellar_bin).unwrap();
    let installed = cellar_bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    write_executable(&fake_bin.join("curl"), "#!/bin/sh\nexit 22\n");
    let cache = home.join(".incodex/cache/update_message");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    fs::write(
        &cache,
        format!(
            "Update {} available, run incodex update\n",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());

    open_menu_long_enough_for_background_refresh(&home, &installed, &path, "q");

    assert_eq!(fs::read_to_string(cache).unwrap(), "");
}

#[test]
fn native_menu_does_not_follow_an_invalid_update_notice_symlink() {
    let home = scratch("menu-notice-symlink");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let (_, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        "#!/bin/sh\nprintf '%s\\n' '{\"tag_name\":\"v9.9.9\"}'\n",
    );
    let cache = home.join(".incodex/cache/update_message");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    let victim = home.join("unrelated-user-file");
    fs::write(&victim, "do not truncate\n").unwrap();
    std::os::unix::fs::symlink(&victim, &cache).unwrap();
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());

    open_menu_long_enough_for_background_refresh(&home, &installed, &path, "q");

    assert_eq!(fs::read_to_string(&victim).unwrap(), "do not truncate\n");
    assert!(!fs::symlink_metadata(cache)
        .unwrap()
        .file_type()
        .is_symlink());
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
