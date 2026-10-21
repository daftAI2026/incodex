#![cfg(target_os = "macos")]
//! [INPUT]: 依赖真实 CLI、私有 HOME 与不存在的目标包。
//! [OUTPUT]: 锁定 Finder 同源 launch 失败的可读回执与只读预览。
//! [POS]: launch 日志接线回归；不启动或修改系统宿主。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
use std::fs;
use std::path::PathBuf;
use std::process::Command;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "incodex-launch-log-route-{}",
            incodex_transaction::new_install_id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn run(&self, extra: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_incodex"))
            .arg("launch")
            .arg("--app")
            .arg(self.0.join("Missing.app"))
            .args(extra)
            .env("HOME", &self.0)
            .env("NO_COLOR", "1")
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn missing_target_failure_has_a_persisted_launch_receipt() {
    let fixture = Fixture::new();
    let output = fixture.run(&[]);
    assert!(!output.status.success(), "a nonexistent app cannot launch");
    let body = fs::read_to_string(fixture.0.join(".incodex/launchers/launch.log"))
        .expect("Finder errors must remain readable after stderr is detached");
    let rows: Vec<serde_json::Value> = body
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["stage"], "requested");
    assert_eq!(rows[1]["stage"], "failed");
    assert_eq!(rows[0]["pid"], rows[1]["pid"]);
    assert_eq!(rows[1]["target"], fixture.0.join("Missing.app").to_str().unwrap());
    assert!(!rows[1]["error"].as_str().unwrap().is_empty());
    assert!(!fixture.0.join("Applications").exists());
    assert!(!fixture.0.join(".incodex/runtime").exists());
}

#[test]
fn failed_preview_and_help_do_not_create_launch_diagnostics() {
    let fixture = Fixture::new();
    let preview = fixture.run(&["--dry-run"]);
    assert!(!preview.status.success());
    let help = fixture.run(&["--help"]);
    assert!(help.status.success());
    assert!(!fixture.0.join(".incodex").exists());
}
