/**
 * [INPUT]: 依赖固定入口路径、文件身份、无覆盖重命名与代际校验器。
 * [OUTPUT]: 提供隔离删除、复验与失败时保留的回滚结果。
 * [POS]: Finder移除器的文件系统边界；产品回调为空，测试回调模拟竞态。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use crate::macos_launch_paths::{rename_noreplace, FileIdentity};
use std::fs;
use std::path::Path;

pub(crate) fn remove_bundle_via_quarantine(
    bundle: &Path,
    quarantine: &Path,
    verify: impl Fn(&Path) -> Result<(), String>,
    after_move: impl FnOnce(&Path, &Path),
) -> Result<(), String> {
    let original_identity = FileIdentity::read(bundle)
        .map_err(|error| format!("cannot record launcher identity before removal: {error}"))?;
    rename_noreplace(bundle, quarantine).map_err(|error| {
        format!(
            "cannot move owned launcher into private removal: {}: {error}",
            bundle.display()
        )
    })?;
    after_move(bundle, quarantine);
    if FileIdentity::read(quarantine).ok() != Some(original_identity) {
        return match rename_noreplace(quarantine, bundle) {
            Ok(()) => Err(format!(
                "launcher changed during removal; original entry restored: {}",
                bundle.display()
            )),
            Err(restore_error) => Err(format!(
                "launcher changed during removal and remains at {}: cannot restore original path: {restore_error}",
                quarantine.display()
            )),
        };
    }
    if let Err(error) = verify(quarantine) {
        return match fs::rename(quarantine, bundle) {
            Ok(()) => Err(format!(
                "launcher changed during removal; original entry restored: {error}"
            )),
            Err(restore_error) => Err(format!(
                "launcher changed during removal and remains at {}: {error}; cannot restore original path: {restore_error}",
                quarantine.display()
            )),
        };
    }
    if let Err(error) = fs::remove_dir_all(quarantine) {
        return match rename_noreplace(quarantine, bundle) {
            Ok(()) => Err(format!(
                "cannot remove owned launcher; original entry restored at {}: {error}",
                bundle.display()
            )),
            Err(restore_error) => Err(format!(
                "cannot remove owned launcher; entry remains at {} and could not return to {}: {error}; {restore_error}",
                quarantine.display(),
                bundle.display()
            )),
        };
    }
    Ok(())
}
