//! [INPUT]: 依赖固定WindowServer窗口证据与按需新鲜CDP目标快照。
//! [OUTPUT]: 预加载观察器；归约ordered-out与空记录加CDP缺席两条Missing路径。
//! [POS]: native open关闭策略；未知重置证据，既有两次Missing规则不变。
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
        confirm_primary_page_absent: impl FnOnce() -> Result<bool, String>,
    ) -> NativeCloseAction {
        let observation = native_window_observation(
            self.observer.observe(pid, primary_bounds),
            confirm_primary_page_absent,
        );
        self.lifecycle.observe(observation)
    }
}

fn native_window_observation(
    observation: OpenWindowObservation,
    confirm_primary_page_absent: impl FnOnce() -> Result<bool, String>,
) -> NativeWindowObservation {
    match observation {
        OpenWindowObservation::Present => NativeWindowObservation::Present,
        OpenWindowObservation::Minimized => NativeWindowObservation::Minimized,
        OpenWindowObservation::Missing => NativeWindowObservation::Missing,
        OpenWindowObservation::PinnedWindowRecordAbsent => match confirm_primary_page_absent() {
            Ok(true) => NativeWindowObservation::Missing,
            Ok(false) | Err(_) => NativeWindowObservation::Unknown,
        },
        OpenWindowObservation::Unknown => NativeWindowObservation::Unknown,
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
    fn pinned_window_record_absence_without_confirmation_keeps_the_session() {
        let mut lifecycle = NativeCloseLifecycle::default();
        let observation =
            native_window_observation(OpenWindowObservation::PinnedWindowRecordAbsent, || {
                Err("fresh CDP snapshot unavailable".into())
            });

        assert_eq!(observation, NativeWindowObservation::Unknown);
        assert_eq!(lifecycle.observe(observation), NativeCloseAction::Keep);
        assert_eq!(lifecycle.observe(observation), NativeCloseAction::Keep);
    }

    #[test]
    fn empty_record_requires_a_fresh_successful_snapshot_without_primary_page() {
        let absence = OpenWindowObservation::PinnedWindowRecordAbsent;

        assert_eq!(
            native_window_observation(absence, || Ok(true)),
            NativeWindowObservation::Missing
        );
        assert_eq!(
            native_window_observation(absence, || Ok(false)),
            NativeWindowObservation::Unknown,
            "a replacement primary page keeps the existing session"
        );
        assert_eq!(
            native_window_observation(absence, || Err("CDP failed".into())),
            NativeWindowObservation::Unknown,
            "a failed target snapshot cannot prove close"
        );
    }

    #[test]
    fn only_empty_window_records_probe_cdp_targets() {
        use std::cell::Cell;

        let probes = Cell::new(0);
        for state in [
            (
                OpenWindowObservation::Present,
                NativeWindowObservation::Present,
            ),
            (
                OpenWindowObservation::Minimized,
                NativeWindowObservation::Minimized,
            ),
            (
                OpenWindowObservation::Missing,
                NativeWindowObservation::Missing,
            ),
            (
                OpenWindowObservation::Unknown,
                NativeWindowObservation::Unknown,
            ),
        ] {
            assert_eq!(
                native_window_observation(state.0, || {
                    probes.set(probes.get() + 1);
                    Ok(true)
                }),
                state.1
            );
        }
        assert_eq!(probes.get(), 0);
    }

    #[test]
    fn confirmed_primary_absence_uses_the_existing_two_sample_close_rule() {
        let mut lifecycle = NativeCloseLifecycle::default();
        assert_eq!(
            lifecycle.observe(NativeWindowObservation::Present),
            NativeCloseAction::Keep
        );
        let missing =
            native_window_observation(OpenWindowObservation::PinnedWindowRecordAbsent, || Ok(true));
        assert_eq!(lifecycle.observe(missing), NativeCloseAction::Keep);
        assert_eq!(lifecycle.observe(missing), NativeCloseAction::Close);
    }

    #[test]
    fn query_errors_and_replacement_pages_reset_the_missing_sequence() {
        let mut lifecycle = NativeCloseLifecycle::default();
        let mut probe_count = 0;

        let observe = |lifecycle: &mut NativeCloseLifecycle,
                       page_absent: Result<bool, String>,
                       probe_count: &mut usize| {
            let observation =
                native_window_observation(OpenWindowObservation::PinnedWindowRecordAbsent, || {
                    *probe_count += 1;
                    page_absent
                });
            lifecycle.observe(observation)
        };

        assert_eq!(
            lifecycle.observe(NativeWindowObservation::Present),
            NativeCloseAction::Keep
        );
        assert_eq!(
            observe(&mut lifecycle, Ok(true), &mut probe_count),
            NativeCloseAction::Keep
        );
        assert_eq!(
            observe(&mut lifecycle, Err("CDP failed".into()), &mut probe_count),
            NativeCloseAction::Keep
        );
        assert_eq!(
            observe(&mut lifecycle, Ok(true), &mut probe_count),
            NativeCloseAction::Keep
        );
        assert_eq!(
            observe(&mut lifecycle, Ok(false), &mut probe_count),
            NativeCloseAction::Keep
        );
        assert_eq!(
            observe(&mut lifecycle, Ok(true), &mut probe_count),
            NativeCloseAction::Keep
        );
        assert_eq!(
            observe(&mut lifecycle, Ok(true), &mut probe_count),
            NativeCloseAction::Close
        );
        assert_eq!(probe_count, 6, "probe each native empty-record observation");
    }
}
