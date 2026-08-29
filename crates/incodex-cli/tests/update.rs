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

fn write_runtime_cli(path: &std::path::Path, version: &str) {
    write_executable(
        path,
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  --version) printf '%s\\n' 'Incodex version {version}' ;;\n  runtime) exit 0 ;;\n  *) exit 88 ;;\nesac\n"
        ),
    );
}

fn installed_cli(home: &std::path::Path) -> (PathBuf, PathBuf) {
    let prefix = home.join("prefix");
    let bin = prefix.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let installed = bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    (prefix, installed)
}

fn homebrew_cli(home: &std::path::Path) -> PathBuf {
    homebrew_cli_generation(home, env!("CARGO_PKG_VERSION"))
}

fn homebrew_cli_generation(home: &std::path::Path, generation: &str) -> PathBuf {
    let bin = home.join(format!("Cellar/incodex/{generation}/bin"));
    fs::create_dir_all(&bin).unwrap();
    let installed = bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    installed
}

fn intel_homebrew_cli(home: &std::path::Path) -> PathBuf {
    let bin = home.join("usr/local/opt/incodex/bin");
    fs::create_dir_all(&bin).unwrap();
    let installed = bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    installed
}

fn usr_local_script_cli(home: &std::path::Path) -> PathBuf {
    let bin = home.join("usr/local/bin");
    fs::create_dir_all(&bin).unwrap();
    let installed = bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    installed
}

#[test]
fn homebrew_update_refreshes_metadata_then_upgrades_through_brew() {
    let home = scratch("homebrew-routing");
    let fake_bin = home.join("fake-bin");
    let brew_log = home.join("brew.log");
    fs::create_dir_all(&fake_bin).unwrap();
    let installed = homebrew_cli(&home);
    let homebrew_prefix = home.join("Cellar/incodex/9.9.9");
    fs::create_dir_all(homebrew_prefix.join("bin")).unwrap();
    write_runtime_cli(&homebrew_prefix.join("bin/incodex"), "9.9.9");
    write_executable(
        &fake_bin.join("brew"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$BREW_LOG\"\ncase \"$*\" in\n  'update') printf '%s\\n' 'simulated metadata failure' >&2; exit 9 ;;\n  'upgrade incodex') printf '%s\\n' 'Upgrading incodex'; exit 0 ;;\n  'list --versions incodex') printf '%s\\n' 'incodex 9.9.9'; exit 0 ;;\n  '--prefix incodex') printf '%s\\n' \"$HOMEBREW_PREFIX\"; exit 0 ;;\n  *) exit 88 ;;\nesac\n",
    );

    let output = Command::new(installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("BREW_LOG", &brew_log)
        .env("HOMEBREW_PREFIX", &homebrew_prefix)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = fs::read_to_string(brew_log).unwrap();
    let calls = calls.lines().collect::<Vec<_>>();
    assert_eq!(&calls[..2], &["update", "upgrade incodex"]);
    assert!(calls.contains(&"list --versions incodex"), "{calls:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("Updated to latest version, 9.9.9"));
}

#[test]
fn homebrew_update_falls_back_to_the_public_cli_version_probe() {
    let home = scratch("homebrew-version-fallback");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let installed = homebrew_cli(&home);
    let homebrew_prefix = home.join("Cellar/incodex/9.9.9");
    fs::create_dir_all(homebrew_prefix.join("bin")).unwrap();
    write_runtime_cli(&homebrew_prefix.join("bin/incodex"), "9.9.9");
    write_executable(
        &fake_bin.join("brew"),
        "#!/bin/sh\ncase \"$*\" in\n  'update'|'upgrade incodex') exit 0 ;;\n  'list --versions incodex') exit 1 ;;\n  '--prefix incodex') printf '%s\\n' \"$HOMEBREW_PREFIX\"; exit 0 ;;\nesac\nexit 88\n",
    );
    write_executable(
        &fake_bin.join("inc"),
        "#!/bin/sh\nprintf '%s\\n' 'Incodex version 9.9.9'\n",
    );

    let output = Command::new(installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("HOMEBREW_PREFIX", &homebrew_prefix)
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Updated to latest version, 9.9.9"));
}

#[test]
fn homebrew_update_lock_is_stable_across_cellar_generations() {
    let home = scratch("homebrew-generation-lock");
    let fake_bin = home.join("fake-bin");
    let update_started = home.join("update-started");
    let update_gate = home.join("update-gate");
    let update_release = home.join("update-release");
    fs::create_dir_all(&fake_bin).unwrap();
    let old_generation = homebrew_cli_generation(&home, "0.5.0");
    let new_generation = homebrew_cli_generation(&home, "0.5.1");
    write_executable(
        &fake_bin.join("brew"),
        "#!/bin/sh\ncase \"$*\" in\n  'update')\n    if mkdir \"$UPDATE_GATE\" 2>/dev/null; then\n      : > \"$UPDATE_STARTED\"\n      while [ ! -f \"$UPDATE_RELEASE\" ]; do sleep 0.05; done\n    fi\n    exit 0 ;;\n  'upgrade incodex') exit 0 ;;\n  'list --versions incodex') printf '%s\\n' 'incodex 0.5.1'; exit 0 ;;\nesac\nexit 88\n",
    );
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());

    let mut first = Command::new(old_generation)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", &path)
        .env("UPDATE_STARTED", &update_started)
        .env("UPDATE_GATE", &update_gate)
        .env("UPDATE_RELEASE", &update_release)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !update_started.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(update_started.exists(), "first updater never entered brew");

    let second = Command::new(new_generation)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", &path)
        .env("UPDATE_STARTED", &update_started)
        .env("UPDATE_GATE", &update_gate)
        .env("UPDATE_RELEASE", &update_release)
        .output()
        .unwrap();
    fs::write(&update_release, "release\n").unwrap();
    let _ = first.wait();

    assert_eq!(second.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&second.stderr).contains("another update is already running"));
}

#[test]
fn homebrew_update_dry_run_previews_both_brew_commands() {
    let home = scratch("homebrew-dry-run");
    let fake_bin = home.join("fake-bin");
    let brew_log = home.join("brew.log");
    fs::create_dir_all(&fake_bin).unwrap();
    let installed = homebrew_cli(&home);
    write_executable(
        &fake_bin.join("brew"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$BREW_LOG\"\nexit 99\n",
    );

    let output = Command::new(installed)
        .args(["update", "--dry-run"])
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("BREW_LOG", &brew_log)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("would run brew update"), "{stdout}");
    assert!(
        stdout.contains("would run brew upgrade incodex"),
        "{stdout}"
    );
    assert!(
        stdout.contains("would publish Runtime with the installed CLI"),
        "{stdout}"
    );
    assert!(!brew_log.exists(), "dry-run executed Homebrew");
}

#[test]
fn intel_homebrew_prefix_uses_the_homebrew_update_path() {
    let home = scratch("intel-homebrew-routing");
    let installed = intel_homebrew_cli(&home);

    let output = Command::new(installed)
        .args(["update", "--dry-run"])
        .env("HOME", &home)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("update channel: homebrew"), "{stdout}");
    assert!(stdout.contains("would run brew update"), "{stdout}");
    assert!(
        stdout.contains("would run brew upgrade incodex"),
        "{stdout}"
    );
}

#[test]
fn intel_homebrew_prefix_reports_the_homebrew_install_channel() {
    let home = scratch("intel-homebrew-version");
    let installed = intel_homebrew_cli(&home);

    let output = Command::new(installed)
        .arg("--version")
        .env("HOME", &home)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Install: Homebrew"), "{stdout}");
}

#[test]
fn usr_local_script_prefix_keeps_the_script_update_path() {
    let home = scratch("usr-local-script-routing");
    let installed = usr_local_script_cli(&home);

    let output = Command::new(installed)
        .args(["update", "--dry-run"])
        .env("HOME", &home)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("update channel: script"), "{stdout}");
    assert!(stdout.contains("would re-run install.sh"), "{stdout}");
    assert!(
        stdout.contains("would publish Runtime with the installed CLI"),
        "{stdout}"
    );
}

#[test]
fn script_update_publishes_runtime_with_the_newly_installed_cli() {
    let home = scratch("script-runtime");
    let fake_bin = home.join("fake-bin");
    let runtime_log = home.join("runtime.log");
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
cat > "$INCODEX_PREFIX/bin/incodex.next" <<'CLI'
#!/bin/sh
case "$1" in
  --version) printf '%s\n' 'Incodex version 9.9.9' ;;
  runtime) printf '%s\n' "$*" > "$RUNTIME_LOG" ;;
  *) exit 88 ;;
esac
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
        .env("RUNTIME_LOG", &runtime_log)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(runtime_log).unwrap().trim(), "runtime");
}

#[test]
fn current_script_update_repairs_runtime_without_downloading_an_installer() {
    let home = scratch("current-runtime-repair");
    let fake_bin = home.join("fake-bin");
    let curl_log = home.join("curl.log");
    fs::create_dir_all(&fake_bin).unwrap();
    let (_, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$CURL_LOG\"\nprintf '{\"tag_name\":\"v%s\"}\\n' \"$CURRENT_VERSION\"\n",
    );

    let output = Command::new(&installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("CURL_LOG", &curl_log)
        .env("CURRENT_VERSION", env!("CARGO_PKG_VERSION"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(home.join(".incodex/runtime/current.json").is_file());
    let urls = fs::read_to_string(curl_log).unwrap();
    assert!(!urls.contains("raw.githubusercontent.com"));
}

#[test]
fn successful_runtime_publication_clears_pending_update_state() {
    let home = scratch("runtime-clears-pending-update");
    let cache = home.join(".incodex/cache");
    let pending = cache.join("runtime_update_pending");
    let notice = cache.join("update_message");
    fs::create_dir_all(&cache).unwrap();
    fs::write(&pending, "pending\n").unwrap();
    fs::write(
        &notice,
        "Runtime synchronization incomplete, run inc update\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .arg("runtime")
        .env("HOME", &home)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!pending.exists(), "successful Runtime left pending state");
    assert_eq!(fs::read_to_string(notice).unwrap(), "");
}

#[test]
fn runtime_failure_reports_partial_success_and_keeps_the_update_notice() {
    let home = scratch("runtime-partial-success");
    let fake_bin = home.join("fake-bin");
    let notice = home.join(".incodex/cache/update_message");
    fs::create_dir_all(&fake_bin).unwrap();
    fs::create_dir_all(notice.parent().unwrap()).unwrap();
    fs::write(&notice, "Update 9.9.9 available, run inc update\n").unwrap();
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
cat > "$INCODEX_PREFIX/bin/incodex.next" <<'CLI'
#!/bin/sh
case "$1" in
  --version) printf '%s\n' 'Incodex version 9.9.9' ;;
  runtime) printf '%s\n' 'simulated Runtime failure' >&2; exit 7 ;;
  *) exit 88 ;;
esac
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
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("CLI was updated"), "{stderr}");
    assert!(stderr.contains("Runtime"), "{stderr}");
    assert!(stderr.contains("simulated Runtime failure"), "{stderr}");
    assert_eq!(
        fs::read_to_string(notice).unwrap(),
        "Runtime synchronization incomplete, run inc update\n"
    );
    assert!(home.join(".incodex/cache/runtime_update_pending").is_file());
}

#[test]
fn homebrew_update_publishes_runtime_from_the_new_cellar_generation() {
    let home = scratch("homebrew-new-runtime");
    let fake_bin = home.join("fake-bin");
    let brew_log = home.join("brew.log");
    let runtime_log = home.join("runtime.log");
    let new_prefix = home.join("Cellar/incodex/9.9.9");
    fs::create_dir_all(&fake_bin).unwrap();
    fs::create_dir_all(new_prefix.join("bin")).unwrap();
    let installed = homebrew_cli(&home);
    write_executable(
        &new_prefix.join("bin/incodex"),
        "#!/bin/sh\ncase \"$1\" in\n  --version) printf '%s\\n' 'Incodex version 9.9.9' ;;\n  runtime) printf '%s\\n' \"$*\" > \"$RUNTIME_LOG\" ;;\n  *) exit 88 ;;\nesac\n",
    );
    write_executable(
        &fake_bin.join("brew"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$BREW_LOG\"\ncase \"$*\" in\n  'update'|'upgrade incodex') exit 0 ;;\n  'list --versions incodex') printf '%s\\n' 'incodex 9.9.9' ;;\n  '--prefix incodex') printf '%s\\n' \"$NEW_PREFIX\" ;;\n  *) exit 88 ;;\nesac\n",
    );

    let output = Command::new(installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("BREW_LOG", &brew_log)
        .env("NEW_PREFIX", &new_prefix)
        .env("RUNTIME_LOG", &runtime_log)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(runtime_log).unwrap().trim(), "runtime");
    let calls = fs::read_to_string(brew_log).unwrap();
    assert!(calls.contains("upgrade incodex\n"), "{calls}");
    assert!(calls.contains("--prefix incodex\n"), "{calls}");
}

#[test]
fn homebrew_update_fails_closed_when_the_new_prefix_cannot_be_resolved() {
    let home = scratch("homebrew-prefix-failure");
    let fake_bin = home.join("fake-bin");
    let fallback_log = home.join("fallback.log");
    fs::create_dir_all(&fake_bin).unwrap();
    let installed = homebrew_cli(&home);
    write_executable(
        &fake_bin.join("brew"),
        "#!/bin/sh\ncase \"$*\" in\n  'update'|'upgrade incodex') exit 0 ;;\n  'list --versions incodex') printf '%s\\n' 'incodex 9.9.9' ;;\n  '--prefix incodex') exit 7 ;;\n  *) exit 88 ;;\nesac\n",
    );
    write_executable(
        &fake_bin.join("incodex"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$FALLBACK_LOG\"\n",
    );

    let output = Command::new(installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("FALLBACK_LOG", &fallback_log)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("CLI was updated"), "{stderr}");
    assert!(stderr.contains("Runtime"), "{stderr}");
    assert!(stderr.contains("Homebrew prefix"), "{stderr}");
    assert!(!fallback_log.exists(), "update fell back to a PATH binary");
    assert!(home.join(".incodex/cache/runtime_update_pending").is_file());
}

#[test]
fn homebrew_update_rejects_a_prefix_cli_that_does_not_match_the_installed_version() {
    let home = scratch("homebrew-prefix-version-mismatch");
    let fake_bin = home.join("fake-bin");
    let new_prefix = home.join("Cellar/incodex/9.9.9");
    let runtime_log = home.join("runtime.log");
    fs::create_dir_all(&fake_bin).unwrap();
    fs::create_dir_all(new_prefix.join("bin")).unwrap();
    let installed = homebrew_cli(&home);
    write_executable(
        &new_prefix.join("bin/incodex"),
        "#!/bin/sh\ncase \"$1\" in\n  --version) printf '%s\\n' 'Incodex version 0.5.0' ;;\n  runtime) : > \"$RUNTIME_LOG\" ;;\n  *) exit 88 ;;\nesac\n",
    );
    write_executable(
        &fake_bin.join("brew"),
        "#!/bin/sh\ncase \"$*\" in\n  'update'|'upgrade incodex') exit 0 ;;\n  'list --versions incodex') printf '%s\\n' 'incodex 9.9.9' ;;\n  '--prefix incodex') printf '%s\\n' \"$NEW_PREFIX\" ;;\n  *) exit 88 ;;\nesac\n",
    );

    let output = Command::new(installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("NEW_PREFIX", &new_prefix)
        .env("RUNTIME_LOG", &runtime_log)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("CLI was updated"), "{stderr}");
    assert!(stderr.contains("did not report 9.9.9"), "{stderr}");
    assert!(!runtime_log.exists(), "mismatched CLI published Runtime");
}

#[test]
fn homebrew_update_preserves_actionable_upgrade_failure() {
    let home = scratch("homebrew-failure");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let installed = homebrew_cli(&home);
    write_executable(
        &fake_bin.join("brew"),
        "#!/bin/sh\ncase \"$*\" in\n  'update') exit 0 ;;\n  'upgrade incodex') printf '%s\\n' 'Please update Xcode before upgrading Incodex.' >&2; exit 7 ;;\nesac\nexit 88\n",
    );

    let output = Command::new(installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Homebrew upgrade failed"), "{stderr}");
    assert!(
        stderr.contains("Please update Xcode before upgrading Incodex."),
        "{stderr}"
    );
}

#[test]
fn homebrew_upgrade_timeout_is_bounded_and_reported() {
    let home = scratch("homebrew-timeout");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let installed = homebrew_cli(&home);
    write_executable(
        &fake_bin.join("brew"),
        "#!/bin/sh\ncase \"$*\" in\n  'update') exit 0 ;;\n  'upgrade incodex') printf '%s\\n' 'Please run xcode-select --install.' >&2; sleep 5; exit 0 ;;\nesac\nexit 88\n",
    );

    let started = Instant::now();
    let output = Command::new(installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("INCODEX_HOMEBREW_UPGRADE_TIMEOUT_MS", "100")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(started.elapsed() < Duration::from_secs(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Homebrew upgrade timed out"));
    assert!(stderr.contains("Please run xcode-select --install."));
}

#[test]
fn homebrew_upgrade_timeout_covers_descendants_holding_output_pipes() {
    let home = scratch("homebrew-descendant-timeout");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let installed = homebrew_cli(&home);
    write_executable(
        &fake_bin.join("brew"),
        "#!/bin/sh\ncase \"$*\" in\n  'update') exit 0 ;;\n  'upgrade incodex') sleep 5 & printf '%s\\n' 'Upgrading incodex'; exit 0 ;;\nesac\nexit 88\n",
    );

    let started = Instant::now();
    let output = Command::new(installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("INCODEX_HOMEBREW_UPGRADE_TIMEOUT_MS", "100")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Homebrew upgrade timed out"));
}

#[test]
fn update_pty_harness_reaps_after_child_closes_before_exit() {
    let home = scratch("pty-close-before-exit");
    let fake_update = home.join("fake-update");
    write_executable(&fake_update, "#!/bin/sh\nexec 0<&- 1>&- 2>&-\nsleep 2\n");

    let (status, _) = run_tty(
        &fake_update,
        &home,
        "/usr/bin:/bin",
        Duration::from_millis(100),
    );

    assert_eq!(status, 124, "update PTY timeout was not reported");
}

fn run_tty(
    program: &std::path::Path,
    home: &std::path::Path,
    path: &str,
    timeout: Duration,
) -> (i32, String) {
    let result = support::tty::run_with_timeout_env(
        program.to_str().unwrap(),
        &["update"],
        &[],
        home,
        "__INCODEX_UPDATE_PROMPT_NEVER__",
        "",
        timeout,
        &[("PATH", path)],
    );
    (result.status, result.stdout)
}

fn visible(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if code.is_ascii() && ('@'..='~').contains(&code) {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
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
    let (status, output) = run_tty(&installed, &home, &path, Duration::from_secs(12));
    assert_eq!(status, 0, "{output:?}");
    let visible_output = visible(&output);
    for stage in [
        "Checking for updates",
        "Downloading stable installer",
        "Installing v9.9.9",
    ] {
        assert!(
            ["|", "/", "-", "\\"]
                .iter()
                .any(|frame| visible_output.contains(&format!("  {frame} {stage}"))),
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
    let (status, output) = run_tty(&installed, &home, &path, Duration::from_secs(12));
    assert_eq!(status, 0, "{output:?}");
    let visible_output = visible(&output);
    for stage in ["Installing v9.9.9", "Repairing v9.9.9"] {
        assert!(
            ["|", "/", "-", "\\"]
                .iter()
                .any(|frame| visible_output.contains(&format!("  {frame} {stage}"))),
            "update stage {stage:?} did not animate: {output:?}"
        );
    }
    assert!(
        output.contains(
            "Stable installer did not complete: update failed: installer exited with exit status: 1: TAGGED-INSTALLER-ERROR"
        ),
        "{output:?}"
    );
    assert_eq!(
        output.matches("TAGGED-INSTALLER-ERROR").count(),
        1,
        "{output:?}"
    );
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
