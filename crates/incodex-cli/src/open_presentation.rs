pub(crate) const DRY_RUN_HEADING: &str = "Open incognito without patching Codex";
pub(crate) const DRY_RUN_COMPLETE: &str = "Dry run. No window opened.";
pub(crate) const OPENING_MESSAGE: &str = "Opening incognito Codex window";
pub(crate) const OPENED_MESSAGE: &str = "Opened. Incognito Codex window is ready.";
#[cfg(not(target_os = "windows"))]
pub(crate) const WAITING_MESSAGE: &str = "Waiting for the window to close";
pub(crate) const CLOSED_REMOVED_MESSAGE: &str = "Closed. Isolated session removed.";
