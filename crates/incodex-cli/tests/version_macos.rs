/*
 * [INPUT]: 依赖真实 CLI、系统只读命令与独立临时 PATH。
 * [OUTPUT]: 验证系统事实不启动子命令，SIP 保留既有查询与未知状态。
 * [POS]: macOS 版本报告回归；只写测试目录，不触碰宿主安装或授权。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#![cfg(target_os = "macos")]

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "incodex-version-test-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).expect("create owned fixture");
        Self(path)
    }

    fn write_probe(&self, name: &str, output: &str) {
        let script = format!(
            "#!/bin/sh\nprintf '%s\\n' \"${{0##*/}}\" >> \"$PROBE_LOG\"\nprintf '%s\\n' '{output}'\n"
        );
        let path = self.0.join(name);
        std::fs::write(&path, script).expect("write fixture probe");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .expect("make fixture executable");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove owned fixture");
    }
}

fn report(path: &Path, log: &Path) -> BTreeMap<String, String> {
    let output = Command::new(env!("CARGO_BIN_EXE_incodex"))
        .arg("--version")
        .env("PATH", path)
        .env("PROBE_LOG", log)
        .env_remove("INCODEX_MACOS_UPDATE_RELAUNCH")
        .env_remove("INCODEX_INTERNAL_UPDATE_NOTICE_WORKER")
        .output()
        .expect("spawn version report");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    String::from_utf8(output.stdout)
        .expect("UTF-8 version report")
        .lines()
        .filter_map(|line| line.split_once(": "))
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

fn system_fact(command: &str, args: &[&str]) -> String {
    let output = Command::new(command).args(args).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn assert_native_facts(facts: &BTreeMap<String, String>) {
    for (label, command, args) in [
        ("macOS", "/usr/bin/sw_vers", &["-productVersion"][..]),
        ("Architecture", "/usr/bin/uname", &["-m"][..]),
        ("Kernel", "/usr/bin/uname", &["-r"][..]),
    ] {
        assert_eq!(facts[label], system_fact(command, args), "{label}");
    }
    let free = facts["Disk Free"]
        .strip_suffix("GB")
        .expect("live disk free is formatted in GB")
        .parse::<f64>()
        .expect("numeric disk free");
    assert!(free.is_finite() && free >= 0.0);
}

#[test]
fn macos_version_reads_native_facts_without_machine_fact_subprocesses() {
    let fixture = Fixture::new();
    let log = fixture.0.join("probes.log");
    for (name, output) in [
        ("sw_vers", "99.9"),
        ("uname", "fixture-machine"),
        (
            "df",
            "Filesystem blocks Used Available\nfixture 1 1 107374182400",
        ),
        ("csrutil", "System Integrity Protection status: disabled."),
    ] {
        fixture.write_probe(name, output);
    }

    let facts = report(&fixture.0, &log);
    assert_eq!(facts["SIP"], "Disabled", "SIP fixture must actually run");
    assert_eq!(std::fs::read_to_string(log).unwrap(), "csrutil\n");
    assert_native_facts(&facts);
    assert_ne!(facts["Disk Free"], "102400.00GB", "ignore fixture df");
}

#[test]
fn macos_version_without_path_keeps_native_facts_and_unknown_sip() {
    let fixture = Fixture::new();
    let log = fixture.0.join("probes.log");
    let facts = report(&fixture.0, &log);
    assert_eq!(facts["SIP"], "Unknown");
    assert_native_facts(&facts);
    assert!(!log.exists());
}
