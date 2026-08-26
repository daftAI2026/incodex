use std::fs;
use std::io;
use std::thread;
use std::time::{Duration, Instant};

use incodex_core::windows_session::{
    burn_windows_session, WindowsCleanupResult, WindowsSessionHome,
};

const CLEANUP_ATTEMPTS: u32 = 5;
const REMOVAL_OBSERVATION: Duration = Duration::from_millis(600);
const REMOVAL_POLL: Duration = Duration::from_millis(100);

pub(crate) fn cleanup_windows_session(session: &WindowsSessionHome) -> WindowsCleanupResult {
    let mut last = WindowsCleanupResult::Unknown {
        reason: "Windows session cleanup was not attempted".to_string(),
    };
    for attempt in 1..=CLEANUP_ATTEMPTS {
        last = burn_windows_session(session);
        if last == WindowsCleanupResult::Removed {
            return observe_removed_session(session);
        }
        if attempt < CLEANUP_ATTEMPTS {
            thread::sleep(Duration::from_millis(200 * u64::from(attempt)));
        }
    }
    last
}

fn observe_removed_session(session: &WindowsSessionHome) -> WindowsCleanupResult {
    let deadline = Instant::now() + REMOVAL_OBSERVATION;
    loop {
        thread::sleep(REMOVAL_POLL);
        match fs::symlink_metadata(&session.root) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if Instant::now() >= deadline {
                    return WindowsCleanupResult::Removed;
                }
            }
            Ok(_) => return burn_windows_session(session),
            Err(error) => {
                return WindowsCleanupResult::Unknown {
                    reason: format!(
                        "cannot observe removed Windows session {}: {error}",
                        session.root.display()
                    ),
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "windows_cleanup_tests.rs"]
mod tests;
