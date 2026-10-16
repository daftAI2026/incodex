/*
 * [INPUT]: 依赖 macOS 精确可执行路径检查和事务 QuiescenceGuard。
 * [OUTPUT]: 提供目标静止检查与官方正常退出适配。
 * [POS]: install 与恢复事务的进程边界；不把一次静止观察缓存为事实。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::path::Path;

use incodex_macos::AppQuiescence;
use incodex_transaction::QuiescenceGuard;

#[derive(Clone)]
pub(crate) struct AppGuard {
    app: Option<AppQuiescence>,
}

impl AppGuard {
    pub(crate) fn for_app(app: &Path) -> Result<Self, String> {
        Ok(Self {
            app: Some(AppQuiescence::for_app(app)?),
        })
    }

    pub(crate) fn for_bundle_at(bundle: &Path, target: &Path) -> Result<Self, String> {
        Ok(Self {
            app: Some(AppQuiescence::for_bundle_at(bundle, target)?),
        })
    }

    pub(crate) fn noop() -> Self {
        Self { app: None }
    }

    pub(crate) fn ensure(&self) -> Result<(), String> {
        self.app
            .as_ref()
            .map_or(Ok(()), AppQuiescence::ensure_quiescent)
    }

    pub(crate) fn close_official(&self) -> Result<(), String> {
        self.app
            .as_ref()
            .map_or(Ok(()), AppQuiescence::quit_official_app_and_wait)
    }
}

impl QuiescenceGuard for AppGuard {
    fn ensure_quiescent(&self, _target: &Path) -> Result<(), String> {
        self.ensure()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_preflight_has_a_closed_busy_outcome_without_string_parsing() {
        let target = Path::new("/tmp/isolated/FakeHost.app");
        let busy = restore_preflight_result(Ok(vec![42]), target).unwrap_err();
        assert_eq!(busy.exit_code(), RESTORE_TARGET_BUSY_EXIT_CODE);
        assert!(busy.message().contains("42"));
        assert!(restore_preflight_result(Ok(vec![]), target).is_ok());
        let probe = restore_preflight_result(Err("probe failed".into()), target).unwrap_err();
        assert_eq!(probe.exit_code(), 1);
        assert_eq!(probe.message(), "probe failed");
    }

    #[test]
    fn restore_preflight_classifies_the_actual_current_process_as_busy() {
        let executable = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        let guard = AppGuard {
            app: Some(AppQuiescence::from_executable(executable).unwrap()),
        };
        let failure = guard.ensure_restore_preflight(Path::new("/tmp/isolated"))
            .unwrap_err();
        assert_eq!(failure.exit_code(), RESTORE_TARGET_BUSY_EXIT_CODE);
        assert!(failure.message().contains(&std::process::id().to_string()));
        assert!(AppGuard::noop().ensure_restore_preflight(Path::new("/tmp/isolated")).is_ok());
    }
}
