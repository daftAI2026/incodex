#![cfg(target_os = "windows")]

// 独立复现公开 Runtime 同步入口遗失授权交接，不依赖新实现的内部测试接口。
use std::fs;
use std::path::PathBuf;

use incodex_cli::windows_install::install_windows_runtime_with;
use incodex_cli::windows_install_state::{
    synchronize_windows_install_runtime_release, WindowsInstallState,
};
use incodex_core::windows_session::apply_private_windows_acl;

const SOURCE: &str = "OpenAI.Codex_1.2.3.4_x64__2p2nqsd0c76g0";
const TARGET: &str = "OpenAI.Codex_1.2.3.5_x64__2p2nqsd0c76g0";

struct Fixture {
    root: PathBuf,
    state: WindowsInstallState,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "incodex-handoff-repro-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let state = install_windows_runtime_with(
            &root,
            SOURCE,
            &std::env::current_exe().unwrap(),
            |_| Ok(vec![]),
            |_| Ok(false),
            |_| Ok(()),
            |_| Ok(()),
        )
        .unwrap();
        Self { root, state }
    }

    fn write_state(&self, value: &serde_json::Value) {
        fs::write(&self.state.state_path, serde_json::to_vec(value).unwrap()).unwrap();
        apply_private_windows_acl(&self.state.state_path).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn current_version_runtime_repair_must_refresh_the_retained_prearm() {
    let fixture = Fixture::new();
    let intent = serde_json::json!({
        "schemaVersion": 1,
        "operationId": "0123456789abcdef0123456789abcdef",
        "sourceRegistrationId": fixture.state.registration_id,
        "sourceEpoch": fixture.state.epoch,
        "sourcePackageFullName": SOURCE,
        "targetPackageFullName": TARGET,
        "helperPath": fixture.state.helper_path,
        "helperSha256": fixture.state.helper_sha256,
        "runtimeRelease": fixture.state.runtime_release,
    });
    let path = fixture.root.join("windows-update-prearm.json");
    fs::write(&path, serde_json::to_vec(&intent).unwrap()).unwrap();
    apply_private_windows_acl(&path).unwrap();
    let mut advanced = serde_json::to_value(&fixture.state).unwrap();
    advanced["epoch"] = (fixture.state.epoch + 3).into();
    fixture.write_state(&advanced);

    let selected =
        synchronize_windows_install_runtime_release(&fixture.root, &fixture.state.runtime_release)
            .unwrap()
            .unwrap();
    let repaired: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(repaired["sourceEpoch"], selected.epoch);
    assert_eq!(repaired["runtimeRelease"], selected.runtime_release);
    assert_eq!(repaired["operationId"], intent["operationId"]);
    assert_eq!(
        repaired["targetPackageFullName"],
        intent["targetPackageFullName"]
    );
}

#[test]
fn runtime_content_update_must_not_revoke_the_live_install_authorization() {
    let fixture = Fixture::new();
    let selected_release = fixture.state.runtime_release.clone();
    let mut old = serde_json::to_value(&fixture.state).unwrap();
    // 此例无 pending，沿用既有安装状态测试对历史 Runtime 名称的构造。
    old["runtimeRelease"] = format!("0.4.0-{}", "1".repeat(64)).into();
    fixture.write_state(&old);
    let selected = synchronize_windows_install_runtime_release(&fixture.root, &selected_release)
        .unwrap()
        .unwrap();
    assert_eq!(selected.epoch, fixture.state.epoch);
    assert_eq!(selected.registration_id, fixture.state.registration_id);
    assert_eq!(selected.runtime_release, selected_release);
}
