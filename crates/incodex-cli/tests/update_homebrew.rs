/*
 * [INPUT]: 依赖原生 CLI、共用更新夹具与替身工具。
 * [OUTPUT]: 验证 Homebrew 路由、安装代际和有界超时输出。
 * 诊断保留断言绑定已发出证据，不要求尚未执行的输出。
 * [POS]: Homebrew 更新集成套件；只执行临时替身 brew。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};

#[path = "update_support/fixture.rs"]
mod fixture;
use fixture::{
    homebrew_cli, homebrew_cli_generation, intel_homebrew_cli, scratch, write_executable,
    write_runtime_cli,
};

const TIMEOUT_HINT: &str = "Please run xcode-select --install.";

fn verify_timeout_detail(stderr: &str, emission_confirmed: bool) -> Result<(), &'static str> {
    if !stderr.contains("Homebrew upgrade timed out") {
        return Err("missing timeout report");
    }
    if emission_confirmed && !stderr.contains(TIMEOUT_HINT) {
        return Err("missing emitted Homebrew diagnostic");
    }
    Ok(())
}

#[test]
fn timeout_detail_requires_confirmed_output_but_not_unemitted_output() {
    let report = "Homebrew upgrade timed out";
    assert!(verify_timeout_detail(report, true).is_err());
    assert!(verify_timeout_detail(TIMEOUT_HINT, true).is_err());
    assert!(verify_timeout_detail(&format!("{report}\n{TIMEOUT_HINT}"), true).is_ok());
    assert!(verify_timeout_detail(report, false).is_ok());
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
    assert!(String::from_utf8_lossy(&output.stdout).contains("🎉 Update ran successfully!"));
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
    assert!(String::from_utf8_lossy(&output.stdout).contains("🎉 Update ran successfully!"));
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
    let emitted = home.join("timeout-diagnostic-emitted");
    fs::create_dir_all(&fake_bin).unwrap();
    let installed = homebrew_cli(&home);
    write_executable(
        &fake_bin.join("brew"),
        &format!("#!/bin/sh\ncase \"$*\" in\n  'update') exit 0 ;;\n  'upgrade incodex') printf '%s\\n' '{TIMEOUT_HINT}' >&2 || exit 91; : > \"$TIMEOUT_EMITTED\" || exit 92; sleep 5; exit 0 ;;\nesac\nexit 88\n"),
    );

    let started = Instant::now();
    let output = Command::new(installed)
        .arg("update")
        .env("HOME", &home)
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("TIMEOUT_EMITTED", &emitted)
        .env("INCODEX_HOMEBREW_UPGRADE_TIMEOUT_MS", "100")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(started.elapsed() < Duration::from_secs(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    // --- 发出证据 ---
    // marker 在 printf 成功后创建。缺少 marker 仍是未知。
    // 进程调度也消耗原有 100ms 预算。
    let emission_confirmed = emitted.is_file();
    assert!(
        verify_timeout_detail(&stderr, emission_confirmed).is_ok(),
        "emission_confirmed={emission_confirmed}, stderr={stderr}"
    );
}

#[test]
fn homebrew_upgrade_timeout_before_output_does_not_invent_a_diagnostic() {
    let home = scratch("homebrew-timeout-before-output");
    let fake_bin = home.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let installed = homebrew_cli(&home);
    write_executable(
        &fake_bin.join("brew"),
        &format!("#!/bin/sh\ncase \"$*\" in\n  'update') exit 0 ;;\n  'upgrade incodex') sleep 5; printf '%s\\n' '{TIMEOUT_HINT}' >&2; exit 0 ;;\nesac\nexit 88\n"),
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
    assert!(!stderr.contains(TIMEOUT_HINT), "{stderr}");
    assert!(verify_timeout_detail(&stderr, false).is_ok(), "{stderr}");
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
