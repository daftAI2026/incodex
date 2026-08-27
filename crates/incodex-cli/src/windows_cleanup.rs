pub(crate) use incodex_core::windows_session::cleanup_windows_session;
use incodex_core::windows_session::{WindowsCleanupResult, WindowsSessionHome};

pub(crate) fn cleanup_windows_session_after_shutdown(
    session: &WindowsSessionHome,
    shutdown: Result<(), String>,
) -> WindowsCleanupResult {
    match shutdown {
        Ok(()) => cleanup_windows_session(session),
        Err(reason) => WindowsCleanupResult::Unknown { reason },
    }
}

#[cfg(test)]
#[path = "windows_cleanup_tests.rs"]
mod tests;
