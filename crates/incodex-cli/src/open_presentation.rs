pub(crate) const DRY_RUN_HEADING: &str = "Open incognito without patching Codex";
pub(crate) const DRY_RUN_COMPLETE: &str = "Dry run. No window opened.";
pub(crate) const OPENING_MESSAGE: &str = "Opening incognito Codex window";
pub(crate) const OPENED_MESSAGE: &str = "Opened. Incognito Codex window is ready.";
pub(crate) const UI_READY_WAIT_MESSAGE: &str = "Waiting for Codex UI to become ready";
pub(crate) const WAITING_MESSAGE: &str = "Waiting for the window to close";
pub(crate) const REMOVING_SESSION_MESSAGE: &str = "Removing isolated session";
pub(crate) const CLOSED_REMOVED_MESSAGE: &str = "Closed. Isolated session removed.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompletedOpenState {
    Success,
    ProcessFailure,
    UiInjectionFailure,
}

pub(crate) fn classify_completed_open(code: i32, ui_ready: bool) -> CompletedOpenState {
    match (code, ui_ready) {
        (0, true) => CompletedOpenState::Success,
        (0, false) => CompletedOpenState::UiInjectionFailure,
        _ => CompletedOpenState::ProcessFailure,
    }
}

pub(crate) fn completed_open_failure_message(code: i32, state: CompletedOpenState) -> String {
    match state {
        CompletedOpenState::Success => String::new(),
        CompletedOpenState::ProcessFailure => {
            format!("Incognito Codex process exited with status {code}")
        }
        CompletedOpenState::UiInjectionFailure => {
            "Incognito Codex UI injection was not accepted".to_string()
        }
    }
}
