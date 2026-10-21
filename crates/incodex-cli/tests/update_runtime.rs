/*
 * [INPUT]: 依赖原生 CLI、共用更新夹具与替身工具。
 * [OUTPUT]: 验证更新后的 Runtime 发布、pending 代际和部分成功。
 * [POS]: 更新与 Runtime 交接回归；不修改实装 Runtime。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs;
use std::process::Command;

#[path = "update_support/fixture.rs"]
mod fixture;
use fixture::{
    embedded_runtime_pending_marker, homebrew_cli, installed_cli, runtime_pending_marker, scratch,
    write_executable,
};

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
    let stdout = String::from_utf8_lossy(&output.stdout);
    for internal in [
        "update channel:",
        "prefix:",
        "updating ",
        "Verified Incodex",
    ] {
        assert!(
            !stdout.contains(internal),
            "internal detail in normal output: {stdout}"
        );
    }
    assert_eq!(
        stdout.trim(),
        "➤ Upgrading Incodex\n➤ Publishing Runtime\n🎉 Updated to version 9.9.9!\nCompatible Runtime updates apply to supported running windows. Other changes load when Codex next starts."
    );
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
fn pending_script_runtime_repair_precedes_a_failed_release_lookup() {
    let home = scratch("pending-script-runtime-repair");
    let fake_bin = home.join("fake-bin");
    let curl_log = home.join("curl.log");
    let cache = home.join(".incodex/cache");
    let pending = cache.join("runtime_update_pending");
    fs::create_dir_all(&fake_bin).unwrap();
    fs::create_dir_all(&cache).unwrap();
    fs::write(&pending, "pending\n").unwrap();
    fs::write(
        cache.join("update_message"),
        "Runtime synchronization incomplete, run inc update\n",
    )
    .unwrap();
    let (_, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$CURL_LOG\"\nexit 7\n",
    );

    let output = Command::new(&installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("CURL_LOG", &curl_log)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(curl_log.is_file(), "update stopped after repairing Runtime");
    assert!(home.join(".incodex/runtime/current.json").is_file());
    assert!(!pending.exists());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("latest release metadata"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn pending_homebrew_runtime_repair_precedes_a_failed_brew_upgrade() {
    let home = scratch("pending-homebrew-runtime-repair");
    let fake_bin = home.join("fake-bin");
    let brew_log = home.join("brew.log");
    let cache = home.join(".incodex/cache");
    let pending = cache.join("runtime_update_pending");
    fs::create_dir_all(&fake_bin).unwrap();
    fs::create_dir_all(&cache).unwrap();
    fs::write(&pending, "pending\n").unwrap();
    fs::write(
        cache.join("update_message"),
        "Runtime synchronization incomplete, run inc update\n",
    )
    .unwrap();
    let installed = homebrew_cli(&home);
    write_executable(
        &fake_bin.join("brew"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$BREW_LOG\"\nexit 7\n",
    );

    let output = Command::new(&installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("BREW_LOG", &brew_log)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let calls = fs::read_to_string(brew_log).unwrap();
    assert!(calls.lines().any(|line| line == "update"), "{calls}");
    assert!(
        calls.lines().any(|line| line == "upgrade incodex"),
        "{calls}"
    );
    assert!(home.join(".incodex/runtime/current.json").is_file());
    assert!(!pending.exists());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Homebrew upgrade failed"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn successful_runtime_publication_clears_pending_update_state() {
    let home = scratch("runtime-clears-pending-update");
    let cache = home.join(".incodex/cache");
    let pending = cache.join("runtime_update_pending");
    let notice = cache.join("update_message");
    fs::create_dir_all(&cache).unwrap();
    fs::write(&pending, embedded_runtime_pending_marker()).unwrap();
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
fn runtime_publication_does_not_clear_a_different_expected_generation() {
    let home = scratch("runtime-preserves-newer-pending-generation");
    let cache = home.join(".incodex/cache");
    let pending = cache.join("runtime_update_pending");
    let notice = cache.join("update_message");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        &pending,
        runtime_pending_marker(
            "9.9.9",
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        ),
    )
    .unwrap();
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

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("expected Runtime generation"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !home.join(".incodex/runtime/current.json").exists(),
        "older Runtime was published over the expected generation"
    );
    assert!(
        pending.is_file(),
        "older Runtime cleared newer pending state"
    );
    assert_eq!(
        fs::read_to_string(notice).unwrap(),
        "Runtime synchronization incomplete, run inc update\n"
    );
}

#[test]
fn runtime_publication_preserves_an_unrelated_cli_update_notice() {
    let home = scratch("runtime-preserves-cli-update");
    let notice = home.join(".incodex/cache/update_message");
    fs::create_dir_all(notice.parent().unwrap()).unwrap();
    fs::write(&notice, "Update 9.9.9 available, run inc update\n").unwrap();

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
    assert_eq!(
        fs::read_to_string(notice).unwrap(),
        "Update 9.9.9 available, run inc update\n"
    );
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
