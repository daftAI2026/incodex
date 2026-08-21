mod support;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

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
    let home = Arc::new((0..128).map(|_| scratch()).collect::<Vec<_>>());
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for home in home.iter().cloned() {
            handles.push(scope.spawn(move || {
                tty::run(
                    env!("CARGO_BIN_EXE_incodex"),
                    &[],
                    &[],
                    &home,
                    "6. Quit",
                    "q",
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
}
