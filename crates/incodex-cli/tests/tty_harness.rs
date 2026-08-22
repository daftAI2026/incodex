mod support;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use support::tty;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "incodex-tty-harness-repro-{}-{seq}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn concurrent_pty_harnesses_report_every_child() {
    const CONTENDERS: usize = 128;
    let home = Arc::new((0..CONTENDERS).map(|_| scratch()).collect::<Vec<_>>());
    let probe = tty::Probe::new(CONTENDERS);
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for home in home.iter() {
            let probe = Arc::clone(&probe);
            handles.push(scope.spawn(move || {
                tty::run_with_probe(
                    env!("CARGO_BIN_EXE_incodex"),
                    &[],
                    &[],
                    home,
                    "6. Quit",
                    "q",
                    &probe,
                )
            }));
        }
        for handle in handles {
            let result = handle.join().unwrap();
            assert_eq!(
                result.status, 0,
                "PTY child was not reported successfully: {result:?}"
            );
            assert!(
                result.stderr.is_empty(),
                "PTY harness emitted diagnostics: {result:?}"
            );
            assert!(
                result.stdout.contains("6. Quit"),
                "PTY child output was truncated: {result:?}"
            );
        }
    });
    assert_eq!(probe.max_concurrency(), 1);
}

#[test]
fn pty_harness_terminates_a_child_when_the_prompt_never_arrives() {
    let home = scratch();
    let started = Instant::now();
    let result = tty::run_with_timeout(
        "/bin/sh",
        &["-c"],
        &["read ignored"],
        &home,
        "prompt that never arrives",
        "q",
        Duration::from_millis(100),
    );

    assert_eq!(
        result.status, 124,
        "PTY timeout was not reported: {result:?}"
    );
    assert!(
        result.stderr.contains("timed out"),
        "PTY timeout lacked diagnostics: {result:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "PTY timeout waited for an unreaped child: {:?}",
        started.elapsed()
    );
}

#[test]
fn pty_harness_kills_a_child_that_closes_the_pty_before_exit() {
    let home = scratch();
    let started = Instant::now();
    let result = tty::run_with_timeout(
        "/bin/sh",
        &["-c"],
        &["exec 0<&- 1>&- 2>&-; sleep 30"],
        &home,
        "prompt that never arrives",
        "q",
        Duration::from_millis(100),
    );

    assert_eq!(
        result.status, 124,
        "PTY timeout was not reported after close: {result:?}"
    );
    assert!(
        result.stderr.contains("timed out"),
        "PTY timeout lacked diagnostics after close: {result:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "PTY timeout waited for a child after the PTY closed: {:?}",
        started.elapsed()
    );
}
