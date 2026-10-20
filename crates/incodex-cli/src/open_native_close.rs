//! [INPUT]: 依赖WindowServer观察器及精确子进程的窗口状态。
//! [OUTPUT]: 提供启动前能力检查与窗口证据到关闭策略的归约。
//! [POS]: native open的关闭策略；不请求CLI或宿主的业务权限。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use incodex_macos::{OpenWindowObservation, OpenWindowObserver};

pub(super) struct NativeCloseMonitor {
    observer: OpenWindowObserver,
    lifecycle: NativeCloseLifecycle,
}

impl NativeCloseMonitor {
    pub(super) fn new() -> Result<Self, String> {
        Self::load_with(OpenWindowObserver::new)
    }

    pub(super) fn load_with(
        load: impl FnOnce() -> Result<OpenWindowObserver, String>,
    ) -> Result<Self, String> {
        Ok(Self {
            observer: load()?,
            lifecycle: NativeCloseLifecycle::default(),
        })
    }

    pub(super) fn needs_primary_bounds(&self) -> bool {
        !self.lifecycle.observed_window
    }

    pub(super) fn observe(
        &mut self,
        pid: i32,
        primary_bounds: Option<incodex_macos::WindowBounds>,
    ) -> NativeCloseAction {
        let observation = native_window_observation(self.observer.observe(pid, primary_bounds));
        self.lifecycle.observe(observation)
    }
}

fn native_window_observation(observation: OpenWindowObservation) -> NativeWindowObservation {
    match observation {
        OpenWindowObservation::Present => NativeWindowObservation::Present,
        OpenWindowObservation::Minimized => NativeWindowObservation::Minimized,
        OpenWindowObservation::Missing => NativeWindowObservation::Missing,
        OpenWindowObservation::PinnedWindowRecordAbsent | OpenWindowObservation::Unknown => {
            NativeWindowObservation::Unknown
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeWindowObservation {
    Present,
    Minimized,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeCloseAction {
    Keep,
    Close,
}

#[derive(Default)]
pub(super) struct NativeCloseLifecycle {
    observed_window: bool,
    consecutive_missing: u8,
}

impl NativeCloseLifecycle {
    pub(super) fn observe(&mut self, state: NativeWindowObservation) -> NativeCloseAction {
        match state {
            NativeWindowObservation::Present | NativeWindowObservation::Minimized => {
                self.observed_window = true;
                self.consecutive_missing = 0;
            }
            NativeWindowObservation::Unknown => self.consecutive_missing = 0,
            NativeWindowObservation::Missing if self.observed_window => {
                self.consecutive_missing = self.consecutive_missing.saturating_add(1);
                if self.consecutive_missing >= 2 {
                    return NativeCloseAction::Close;
                }
            }
            NativeWindowObservation::Missing => {}
        }
        NativeCloseAction::Keep
    }
}

#[cfg(test)]
mod tests {
    use super::{
        native_window_observation, NativeCloseAction, NativeCloseLifecycle, NativeWindowObservation,
    };
    use incodex_macos::OpenWindowObservation;

    #[test]
    fn pinned_window_record_absence_alone_keeps_the_session() {
        let mut lifecycle = NativeCloseLifecycle::default();
        let observation =
            native_window_observation(OpenWindowObservation::PinnedWindowRecordAbsent);

        assert_eq!(observation, NativeWindowObservation::Unknown);
        assert_eq!(lifecycle.observe(observation), NativeCloseAction::Keep);
        assert_eq!(lifecycle.observe(observation), NativeCloseAction::Keep);
    }
}
