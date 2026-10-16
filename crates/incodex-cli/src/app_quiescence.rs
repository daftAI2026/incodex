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
