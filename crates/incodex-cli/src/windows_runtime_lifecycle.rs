#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsRuntimeStartupAction {
    Continue,
    Finish,
    FailExited,
}

pub fn windows_runtime_startup_action(
    authenticated_close: bool,
    job_empty: bool,
) -> WindowsRuntimeStartupAction {
    if authenticated_close {
        WindowsRuntimeStartupAction::Finish
    } else if job_empty {
        WindowsRuntimeStartupAction::FailExited
    } else {
        WindowsRuntimeStartupAction::Continue
    }
}

pub fn windows_runtime_ready_for_handshake(runtime_accepted: bool, visible: bool) -> bool {
    runtime_accepted && visible
}

pub fn windows_runtime_shutdown_authorized(authenticated_close: bool, job_empty: bool) -> bool {
    authenticated_close || job_empty
}
