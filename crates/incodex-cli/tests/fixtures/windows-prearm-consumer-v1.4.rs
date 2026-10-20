// 冻结 v1.4.0 / 849da3476f5bc25731b9c276ab9e4d2863d7c083 的 schema 与授权条件。
// 只用于新发布器到旧读端的兼容回归，不参与产品执行或代替真实 Store 生命周期。
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum WindowsInstallPhase {
    Staged,
    EnablePending,
    EnabledUnobserved,
    EnabledObserved,
    DisableRequested,
    DisablePending,
    Disabled,
    RecoveryRequired,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum WindowsInstallDesired {
    Enabled,
    Disabled,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowsInstallState {
    schema_version: u32,
    epoch: u64,
    registration_id: String,
    desired: WindowsInstallDesired,
    phase: WindowsInstallPhase,
    package_full_name: String,
    helper_path: PathBuf,
    helper_sha256: String,
    runtime_release: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowsUpdateRepairIntent {
    schema_version: u32,
    operation_id: String,
    source_registration_id: String,
    source_epoch: u64,
    source_package_full_name: String,
    target_package_full_name: String,
    helper_path: PathBuf,
    helper_sha256: String,
    runtime_release: String,
}

impl WindowsUpdateRepairIntent {
    fn authorizes_source(&self, source: &WindowsInstallState, target: &str) -> bool {
        source.desired == WindowsInstallDesired::Enabled
            && matches!(
                source.phase,
                WindowsInstallPhase::EnabledObserved | WindowsInstallPhase::EnabledUnobserved
            )
            && source.package_full_name == self.source_package_full_name
            && source.epoch == self.source_epoch
            && source.registration_id == self.source_registration_id
            && source.helper_path == self.helper_path
            && source.helper_sha256 == self.helper_sha256
            && source.runtime_release == self.runtime_release
            && self.target_package_full_name == target
            && source.package_full_name != target
    }
}

pub(super) fn accepts(source: &[u8], intent: &[u8], target: &str) -> bool {
    let source: WindowsInstallState = serde_json::from_slice(source).unwrap();
    let intent: WindowsUpdateRepairIntent = serde_json::from_slice(intent).unwrap();
    source.schema_version == 1
        && intent.schema_version == 1
        && intent.operation_id.len() == 32
        && intent.authorizes_source(&source, target)
}
