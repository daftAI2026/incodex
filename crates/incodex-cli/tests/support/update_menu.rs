use std::fs;
use std::process::Command;

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
