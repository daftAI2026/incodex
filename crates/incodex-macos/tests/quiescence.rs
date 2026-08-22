use std::path::PathBuf;
use std::time::{Duration, Instant};

use incodex_macos::{AppQuiescence, ProcessProbe, QuiescenceClock, QuitRequester};

struct EmptyProbe;

impl ProcessProbe for EmptyProbe {
    fn process_paths(&self) -> Result<Vec<(i32, PathBuf)>, String> {
        Ok(Vec::new())
    }
}

struct MustNotQuit;

impl QuitRequester for MustNotQuit {
    fn request_quit(&mut self) -> Result<(), String> {
        Err("requester must not be called when no exact PID is live".into())
    }
}

struct Clock(Instant);

impl QuiescenceClock for Clock {
    fn now(&self) -> Instant {
        self.0
    }

    fn sleep(&mut self, duration: Duration) {
        self.0 += duration;
    }
}

#[test]
fn official_quit_skips_applescript_when_no_exact_pid_is_live() {
    let quiescence = AppQuiescence::from_executable(PathBuf::from(
        "/tmp/incodex/ChatGPT.app/Contents/MacOS/ChatGPT",
    ))
    .unwrap();
    let mut requester = MustNotQuit;
    let mut clock = Clock(Instant::now());

    quiescence
        .quit_official_app_and_wait_with(&EmptyProbe, &mut requester, &mut clock)
        .unwrap();
}

#[test]
fn system_probe_observes_the_current_executable_by_exact_path() {
    let executable = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let quiescence = AppQuiescence::from_executable(executable).unwrap();

    let error = quiescence.ensure_quiescent().unwrap_err();

    assert!(error.contains(&std::process::id().to_string()), "{error}");
}
