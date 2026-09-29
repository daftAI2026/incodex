//! Decision boundary for the macOS window that owns a native `open` session.

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

pub(super) struct NativeCloseLifecycle {
    observed_window: bool,
    consecutive_missing: u8,
}

impl NativeCloseLifecycle {
    pub(super) fn new(accessibility_trusted: bool) -> Result<Self, String> {
        if !accessibility_trusted {
            return Err("incodex open needs Accessibility access for the terminal or launcher running this command to distinguish a closed window from a minimized window. Add or enable the requesting app in System Settings > Privacy & Security > Accessibility (macOS 13+) or System Preferences > Security & Privacy > Privacy > Accessibility (macOS 12), then retry. This check failed before a session is created".into());
        }
        Ok(Self {
            observed_window: false,
            consecutive_missing: 0,
        })
    }

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
