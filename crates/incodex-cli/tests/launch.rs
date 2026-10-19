#![cfg(target_os = "macos")]
//! [INPUT]: 依赖真实CLI与临时假App；不启动系统宿主。
//! [OUTPUT]: 约束普通CDP启动入口的帮助、预览与无效目标行为。
//! [POS]: launch的外部命令合同；隔离测试不替代真实App验收。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "incodex-launch-contract-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_incodex"))
            .args(args)
            .env("HOME", &self.0)
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .output()
            .unwrap()
    }
    fn app(&self) -> PathBuf {
        let app = self.0.join("Official Host.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(app.join("Contents/Info.plist"),
            "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>Host</string></dict></plist>").unwrap();
        let executable = app.join("Contents/MacOS/Host");
        fs::write(
            &executable,
            "#!/bin/sh\nprintf 'must not run' > \"$HOME/host-started\"\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        app
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
#[test]
fn launch_help_describes_normal_cdp_without_installing() {
    let fixture = Fixture::new();
    let output = fixture.run(&["launch", "--help"]);
    assert!(output.status.success(), "{}", text(&output));
    let help = text(&output);
    assert!(help.contains("normal") && help.contains("CDP"), "{help}");
    assert!(!fixture.0.join(".incodex").exists());
}
#[test]
fn launch_dry_run_keeps_official_app_and_home_untouched() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let executable = app.join("Contents/MacOS/Host");
    let before = fs::read(&executable).unwrap();
    let output = fixture.run(&["launch", "--dry-run", "--app", app.to_str().unwrap()]);
    assert!(output.status.success(), "{}", text(&output));
    let plan = text(&output);
    assert!(
        plan.contains("CDP") && plan.contains(app.to_str().unwrap()),
        "{plan}"
    );
    assert_eq!(fs::read(&executable).unwrap(), before);
    assert!(!fixture.0.join("host-started").exists());
    assert!(!fixture.0.join(".incodex").exists());
}
#[test]
fn launch_missing_target_fails_before_creating_state() {
    let fixture = Fixture::new();
    let missing = fixture.0.join("Missing.app");
    let output = fixture.run(&["launch", "--dry-run", "--app", missing.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(
        !text(&output).contains("unknown command: launch"),
        "{}",
        text(&output)
    );
    assert!(!fixture.0.join(".incodex").exists());
    assert!(!Path::new(&missing).exists());
}

#[test]
fn launch_rejects_mutation_and_unimplemented_output_flags() {
    let fixture = Fixture::new();
    for flag in ["--clone", "--live", "--yes", "--json", "--restore-app"] {
        let output = fixture.run(&["launch", "--dry-run", flag]);
        assert!(!output.status.success(), "{flag}: {}", text(&output));
        assert!(
            text(&output).contains("launch accepts only"),
            "{flag}: {}",
            text(&output)
        );
        assert!(!fixture.0.join(".incodex").exists());
    }
}
