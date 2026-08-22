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
