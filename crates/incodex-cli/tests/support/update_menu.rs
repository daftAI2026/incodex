use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};

use super::support;
use super::{installed_cli, scratch, write_executable};

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
        "Update 9.9.9 available, run inc update"
    );
}

#[test]
fn native_menu_refresh_survives_an_immediate_exit() {
    let home = scratch("menu-detached-refresh");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let (_, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        "#!/bin/sh\nsleep 0.5\nprintf '%s\\n' '{\"tag_name\":\"v9.9.9\"}'\n",
    );
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());

    let result = support::tty::run_with_timeout_env(
        installed.to_str().unwrap(),
        &[],
        &[],
        &home,
        "6. Quit",
        "q",
        Duration::from_secs(3),
        &[("PATH", path.as_str())],
    );
    assert_eq!(result.status, 0, "{}", result.stderr);

    let cache = home.join(".incodex/cache/update_message");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !cache.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        fs::read_to_string(cache).unwrap().trim(),
        "Update 9.9.9 available, run inc update"
    );
}

#[test]
fn native_menu_preserves_a_runtime_synchronization_notice() {
    let home = scratch("menu-runtime-pending");
    let fake_bin = home.join("fake-bin");
    let curl_called = home.join("curl-called");
    fs::create_dir_all(&fake_bin).unwrap();
    let (_, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        "#!/bin/sh\n: > \"$CURL_CALLED\"\nexit 22\n",
    );
    let cache = home.join(".incodex/cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("runtime_update_pending"), "pending\n").unwrap();
    fs::write(
        cache.join("update_message"),
        "Update 9.9.9 available, run inc update\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());
    let curl_called_text = curl_called.to_string_lossy();

    let result = support::tty::run_with_timeout_env(
        installed.to_str().unwrap(),
        &[],
        &[],
        &home,
        "Runtime synchronization incomplete, run inc update",
        "q",
        Duration::from_secs(3),
        &[
            ("PATH", path.as_str()),
            ("CURL_CALLED", curl_called_text.as_ref()),
        ],
    );

    assert_eq!(result.status, 0, "{}", result.stderr);
    std::thread::sleep(Duration::from_millis(200));
    assert!(!curl_called.exists(), "pending Runtime state was refreshed away");
    assert!(cache.join("runtime_update_pending").is_file());
}

#[test]
fn detached_old_cli_refresh_cannot_restore_a_notice_after_the_cli_changes() {
    let home = scratch("menu-stale-worker");
    let fake_bin = home.join("fake-bin");
    let curl_called = home.join("curl-called");
    fs::create_dir_all(&fake_bin).unwrap();
    let (_, installed) = installed_cli(&home);
    write_executable(
        &fake_bin.join("curl"),
        "#!/bin/sh\n: > \"$CURL_CALLED\"\nsleep 0.5\nprintf '%s\\n' '{\"tag_name\":\"v9.9.9\"}'\n",
    );
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());
    let curl_called_text = curl_called.to_string_lossy();

    let result = support::tty::run_with_timeout_env(
        installed.to_str().unwrap(),
        &[],
        &[],
        &home,
        "6. Quit",
        "q",
        Duration::from_secs(3),
        &[
            ("PATH", path.as_str()),
            ("CURL_CALLED", curl_called_text.as_ref()),
        ],
    );
    assert_eq!(result.status, 0, "{}", result.stderr);

    let replacement = installed.with_extension("next");
    write_executable(
        &replacement,
        "#!/bin/sh\ncase \"$1\" in\n  --version) printf '%s\\n' 'Incodex version 9.9.9' ;;\n  *) exit 88 ;;\nesac\n",
    );
    fs::rename(replacement, &installed).unwrap();

    let cache = home.join(".incodex/cache/update_message");
    let deadline = Instant::now() + Duration::from_secs(3);
    while (!curl_called.exists() || !cache.exists()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(curl_called.exists(), "detached refresh did not run");
    assert_eq!(fs::read_to_string(cache).unwrap(), "");
}

#[test]
fn native_menu_clears_a_stale_notice_when_release_lookup_fails() {
    let home = scratch("menu-failed-refresh");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let (_, installed) = installed_cli(&home);
    let curl_called = home.join("curl-called");
    write_executable(
        &fake_bin.join("curl"),
        "#!/bin/sh\n: > \"$CURL_CALLED\"\nexit 22\n",
    );
    let cache = home.join(".incodex/cache/update_message");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    fs::write(&cache, "Update 9.9.9 available, run inc update\n").unwrap();
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());
    let curl_called_text = curl_called.to_string_lossy();

    let result = support::tty::run_with_timeout_env(
        installed.to_str().unwrap(),
        &[],
        &[],
        &home,
        "6. Quit",
        "q",
        Duration::from_secs(3),
        &[
            ("PATH", path.as_str()),
            ("CURL_CALLED", curl_called_text.as_ref()),
        ],
    );
    assert_eq!(result.status, 0, "{}", result.stderr);

    let deadline = Instant::now() + Duration::from_secs(3);
    while !curl_called.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        curl_called.exists(),
        "background release lookup did not run"
    );
    assert_eq!(fs::read_to_string(cache).unwrap(), "");
}

#[test]
fn native_homebrew_menu_waits_for_the_formula_and_names_inc_update() {
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
        "#!/bin/sh\nif [ \"$*\" = 'outdated --formula --verbose incodex' ]; then printf '%s\\n' 'incodex (0.3.1) < 9.9.9'; fi\n",
    );
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());

    open_menu_long_enough_for_background_refresh(&home, &installed, &path, "q");

    let cache = home.join(".incodex/cache/update_message");
    assert_eq!(
        fs::read_to_string(cache).unwrap().trim(),
        "Update 9.9.9 available, run inc update"
    );
}

#[test]
fn native_homebrew_menu_requires_a_new_stable_release_before_formula_lookup() {
    let home = scratch("menu-homebrew-release-gate");
    let fake_bin = home.join("fake-bin");
    let cellar_bin = home.join(format!("Cellar/incodex/{}/bin", env!("CARGO_PKG_VERSION")));
    fs::create_dir_all(&fake_bin).unwrap();
    fs::create_dir_all(&cellar_bin).unwrap();
    let installed = cellar_bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    write_executable(
        &fake_bin.join("curl"),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' '{{\"tag_name\":\"v{}\"}}'\n",
            env!("CARGO_PKG_VERSION")
        ),
    );
    let brew_log = home.join("brew.log");
    write_executable(
        &fake_bin.join("brew"),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nprintf '%s\\n' 'incodex ({}) < 9.9.9'\n",
            brew_log.display(),
            env!("CARGO_PKG_VERSION")
        ),
    );
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());

    open_menu_long_enough_for_background_refresh(&home, &installed, &path, "q");

    let cache = home.join(".incodex/cache/update_message");
    assert_eq!(fs::read_to_string(cache).unwrap(), "");
    assert!(
        !brew_log.exists(),
        "formula lookup ran without a new release"
    );
}

#[test]
fn native_homebrew_menu_exposes_the_unified_update_shortcut() {
    let home = scratch("menu-homebrew-no-self-update");
    let fake_bin = home.join("fake-bin");
    let cellar_bin = home.join(format!("Cellar/incodex/{}/bin", env!("CARGO_PKG_VERSION")));
    fs::create_dir_all(&fake_bin).unwrap();
    fs::create_dir_all(&cellar_bin).unwrap();
    let installed = cellar_bin.join("incodex");
    fs::copy(env!("CARGO_BIN_EXE_incodex"), &installed).unwrap();
    let cellar_prefix = cellar_bin.parent().unwrap();
    let brew_log = home.join("brew.log");
    write_executable(&fake_bin.join("curl"), "#!/bin/sh\nexit 22\n");
    write_executable(
        &fake_bin.join("brew"),
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$*\" in\n  'update') exit 1 ;;\n  'upgrade incodex') printf '%s\\n' 'incodex 9.9.9 already installed'; exit 0 ;;\n  'list --versions incodex') printf '%s\\n' 'incodex 9.9.9'; exit 0 ;;\n  '--prefix incodex') printf '%s\\n' '{}'; exit 0 ;;\nesac\n",
            brew_log.display(),
            cellar_prefix.display()
        ),
    );
    let cache = home.join(".incodex/cache/update_message");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    fs::write(cache, "Update 9.9.9 available, run inc update\n").unwrap();
    let path = format!("{}:/usr/bin:/bin", fake_bin.display());

    open_menu_long_enough_for_background_refresh(&home, &installed, &path, "uq");

    let calls = fs::read_to_string(brew_log).unwrap();
    assert!(calls.lines().any(|line| line == "update"), "{calls}");
    assert!(
        calls.lines().any(|line| line == "upgrade incodex"),
        "{calls}"
    );
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
            "Update {} available, run inc update\n",
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
