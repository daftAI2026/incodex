use std::fs;
use std::thread;
use std::time::{Duration, Instant};

use incodex_core::windows_session::{create_windows_session, WindowsCleanupResult};

#[test]
fn cleanup_is_skipped_when_job_shutdown_cannot_be_proven() {
    let fixture = std::env::temp_dir().join(format!(
        "incodex-cleanup-unproven-shutdown-{}",
        std::process::id()
    ));
    let user_root = fixture.join(".incodex");
    fs::create_dir_all(&user_root).expect("create cleanup user root");
    let session = create_windows_session(&user_root).expect("create cleanup fixture");
    let session_root = session.root.clone();

    let result = super::cleanup_windows_session_after_shutdown(
        &session,
        Err("fixture could not prove the Windows Job was empty".to_string()),
    );

    assert!(matches!(
        result,
        WindowsCleanupResult::Unknown { ref reason }
            if reason.contains("could not prove the Windows Job was empty")
    ));
    assert!(
        session_root.exists(),
        "cleanup must not touch a session that may still have live writers"
    );
    fs::remove_dir_all(fixture).expect("remove retained cleanup fixture");
}

#[test]
fn cleanup_observes_a_session_recreated_after_initial_removal() {
    let fixture = std::env::temp_dir().join(format!(
        "incodex-cleanup-observation-{}",
        std::process::id()
    ));
    let user_root = fixture.join(".incodex");
    fs::create_dir_all(&user_root).expect("create cleanup user root");
    let session = create_windows_session(&user_root).expect("create cleanup fixture");
    let session_root = session.root.clone();
    let recreated_root = session_root.clone();
    let recreator = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        while recreated_root.exists() && Instant::now() < deadline {
            thread::yield_now();
        }
        assert!(
            !recreated_root.exists(),
            "initial cleanup never removed session"
        );
        fs::create_dir_all(recreated_root.join("late-writer"))
            .expect("recreate session after initial removal");
    });

    let result = super::cleanup_windows_session(&session);
    recreator.join().expect("join late writer");

    assert!(
        matches!(result, WindowsCleanupResult::Retained { .. }),
        "a recreated session must not be reported as removed: {result:?}"
    );
    assert!(session_root.exists(), "unsafe recreated data was deleted");
    fs::remove_dir_all(fixture).expect("remove cleanup fixture");
}
