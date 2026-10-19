#![cfg(target_os = "macos")]
/*
 * [INPUT]: 依赖真实 CLI 与隔离 HOME；仅提交未获权的内部任务。
 * [OUTPUT]: 验证更新权限任务先于公共解析且拒绝伪造登记。
 * [POS]: 更新接线回归；不运行宿主、不读真实 TCC 或用户数据。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "incodex-permission-update-test-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn cli(home: &Home) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_incodex"));
    command
        .env("HOME", &home.0)
        .env_remove("INCODEX_MACOS_UPDATE_RELAUNCH")
        .env_remove("INCODEX_INTERNAL_UPDATE_NOTICE_WORKER")
        .env_remove("INCODEX_INTERNAL_PERMISSION_UPDATE");
    command
}
#[test]
fn malformed_permission_update_is_rejected_before_public_help_without_state() {
    let home = Home::new();
    let output = cli(&home)
        .arg("--help")
        .env("INCODEX_INTERNAL_PERMISSION_UPDATE", "not-json")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "malformed internal task fell through to public parsing"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("permission update task"));
    assert!(
        !home.0.join(".incodex").exists(),
        "untrusted task created state"
    );
}
#[test]
fn valid_looking_permission_update_cannot_authorize_itself() {
    let home = Home::new();
    let request = serde_json::json!({"schemaVersion":1,"installId":"0f8f4baa-12a1-4533-a54a-7b58c8a341c2", "helperSha256":"a".repeat(64), "runtimeSha256":"b".repeat(64), "source":"runtime"});
    let output = cli(&home)
        .arg("--help")
        .env("INCODEX_INTERNAL_PERMISSION_UPDATE", request.to_string())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("registration"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !home.0.join(".incodex").exists(),
        "unsigned task created state"
    );
}
#[test]
fn ordinary_help_still_succeeds_without_a_permission_update_task() {
    let home = Home::new();
    let output = cli(&home).arg("--help").output().unwrap();
    assert!(output.status.success());
    assert!(!home.0.join(".incodex").exists());
}
#[test]
fn runtime_preview_explains_conditional_permission_check_without_mutation() {
    let home = Home::new();
    let output = cli(&home).args(["runtime", "--dry-run"]).output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("check current ChatGPT permissions"));
    assert!(!home.0.join(".incodex").exists());
}
