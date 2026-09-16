//! Read-only Accessibility/TCC inspection for the exact executable of a live app.
//!
//! The implementation is intentionally left for the follow-up implementation change.  The
//! tests below define the safety contract: process identity is exact and ambiguity is unknown;
//! a missing or failed TCC query must never become a denial or a grant.

#[cfg(test)]
mod tests {
    use super::{
        inspect_accessibility_for_app, inspect_accessibility_for_app_with, AccessibilityStatus,
        TccAccessibilityProbe,
    };
    use super::super::ProcessProbe;
    use std::collections::VecDeque;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Clone)]
    struct FixtureProcessProbe {
        snapshots: Arc<Mutex<VecDeque<Result<Vec<(i32, PathBuf)>, String>>>>,
    }

    impl FixtureProcessProbe {
        fn new(snapshots: Vec<Result<Vec<(i32, PathBuf)>, String>>) -> Self {
            Self {
                snapshots: Arc::new(Mutex::new(snapshots.into_iter().collect())),
            }
        }
    }

    impl ProcessProbe for FixtureProcessProbe {
        fn process_paths(&self) -> Result<Vec<(i32, PathBuf)>, String> {
            self.snapshots
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("fixture process snapshot exhausted".into()))
        }
    }

    #[derive(Clone)]
    enum FixtureTccOutcome {
        Missing,
        Failed,
        Decision(bool),
    }

    #[derive(Clone)]
    struct FixtureTcc {
        outcome: FixtureTccOutcome,
        calls: Arc<Mutex<Vec<i32>>>,
    }

    impl FixtureTcc {
        fn missing() -> Self {
            Self::new(FixtureTccOutcome::Missing)
        }

        fn failed() -> Self {
            Self::new(FixtureTccOutcome::Failed)
        }

        fn decision(value: bool) -> Self {
            Self::new(FixtureTccOutcome::Decision(value))
        }

        fn new(outcome: FixtureTccOutcome) -> Self {
            Self {
                outcome,
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn calls(&self) -> Vec<i32> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl TccAccessibilityProbe for FixtureTcc {
        fn check_accessibility(&self, pid: i32) -> Result<bool, String> {
            self.calls.lock().unwrap().push(pid);
            match &self.outcome {
                FixtureTccOutcome::Missing => {
                    Err("TCCAccessCheckAuditToken is unavailable".into())
                }
                FixtureTccOutcome::Failed => Err("TCC access check failed".into()),
                FixtureTccOutcome::Decision(value) => Ok(*value),
            }
        }
    }

    fn app_fixture() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "incodex-accessibility-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let app = root.join("ChatGPT.app");
        let executable = app.join("Contents/MacOS/ChatGPT");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.openai.codex</string>
<key>CFBundleExecutable</key><string>ChatGPT</string>
</dict></plist>
"#,
        )
        .unwrap();
        fs::write(&executable, b"fixture executable").unwrap();
        (app, executable)
    }

    fn stable_probe(pid: i32, executable: &Path) -> FixtureProcessProbe {
        let snapshot = vec![(pid, executable.to_path_buf())];
        FixtureProcessProbe::new(vec![Ok(snapshot.clone()), Ok(snapshot)])
    }

    fn assert_unknown(report: &super::AccessibilityReport) {
        assert_eq!(report.status, AccessibilityStatus::Unknown);
        assert!(report.reason.is_some(), "unknown result must explain why");
    }

    #[test]
    fn no_exactly_matching_running_host_is_not_running() {
        let (app, _executable) = app_fixture();
        let probe = FixtureProcessProbe::new(vec![Ok(Vec::new())]);
        let tcc = FixtureTcc::decision(true);

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_eq!(report.status, AccessibilityStatus::NotRunning);
        assert_eq!(report.pid, None);
        assert!(tcc.calls().is_empty());
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn same_basename_in_another_bundle_is_not_running() {
        let (app, _executable) = app_fixture();
        let probe = FixtureProcessProbe::new(vec![Ok(vec![
            (41, PathBuf::from("/tmp/another/ChatGPT.app/Contents/MacOS/ChatGPT")),
        ])]);
        let tcc = FixtureTcc::decision(true);

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_eq!(report.status, AccessibilityStatus::NotRunning);
        assert_eq!(report.pid, None);
        assert!(tcc.calls().is_empty());
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn multiple_exact_hosts_fail_closed_without_randomly_checking_one() {
        let (app, executable) = app_fixture();
        let probe = FixtureProcessProbe::new(vec![Ok(vec![
            (41, executable.clone()),
            (42, executable.clone()),
        ])]);
        let tcc = FixtureTcc::decision(true);

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_unknown(&report);
        assert_eq!(report.pid, None);
        assert!(tcc.calls().is_empty());
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn duplicate_pid_or_reused_pid_between_snapshots_fails_closed() {
        let (app, executable) = app_fixture();
        let other_executable = executable.with_file_name("OtherHost");
        let probe = FixtureProcessProbe::new(vec![
            Ok(vec![(41, executable.clone())]),
            Ok(vec![(41, other_executable)]),
        ]);
        let tcc = FixtureTcc::decision(true);

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_unknown(&report);
        assert_eq!(report.pid, None, "a reused PID is no longer a trusted identity");
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn duplicate_pid_entries_are_ambiguous_even_when_the_path_matches() {
        let (app, executable) = app_fixture();
        let probe = FixtureProcessProbe::new(vec![Ok(vec![
            (41, executable.clone()),
            (41, executable.clone()),
        ])]);
        let tcc = FixtureTcc::decision(true);

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_unknown(&report);
        assert_eq!(report.pid, None);
        assert!(tcc.calls().is_empty());
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn missing_tcc_symbol_is_unknown() {
        let (app, executable) = app_fixture();
        let probe = stable_probe(41, &executable);
        let tcc = FixtureTcc::missing();

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_unknown(&report);
        assert_eq!(report.pid, Some(41));
        assert_eq!(tcc.calls(), vec![41]);
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn tcc_call_failure_is_unknown() {
        let (app, executable) = app_fixture();
        let probe = stable_probe(41, &executable);
        let tcc = FixtureTcc::failed();

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_unknown(&report);
        assert_eq!(report.pid, Some(41));
        assert_eq!(tcc.calls(), vec![41]);
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn explicit_tcc_false_is_denied() {
        let (app, executable) = app_fixture();
        let probe = stable_probe(41, &executable);
        let tcc = FixtureTcc::decision(false);

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_eq!(report.status, AccessibilityStatus::Denied);
        assert_eq!(report.pid, Some(41));
        assert_eq!(tcc.calls(), vec![41]);
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn explicit_tcc_true_is_granted() {
        let (app, executable) = app_fixture();
        let probe = stable_probe(41, &executable);
        let tcc = FixtureTcc::decision(true);

        let report = inspect_accessibility_for_app_with(&app, &probe, &tcc);

        assert_eq!(report.status, AccessibilityStatus::Granted);
        assert_eq!(report.pid, Some(41));
        assert_eq!(tcc.calls(), vec![41]);
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[test]
    fn process_probe_failure_is_unknown() {
        let (app, _executable) = app_fixture();
        let probe = FixtureProcessProbe::new(vec![Err("process table unavailable".into())]);

        let report = inspect_accessibility_for_app_with(&app, &probe, &FixtureTcc::decision(true));

        assert_unknown(&report);
        assert_eq!(report.pid, None);
        let _ = fs::remove_dir_all(app.parent().unwrap());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_is_explicitly_unsupported_as_unknown() {
        let report = inspect_accessibility_for_app(Path::new("/tmp/does-not-matter.app"));

        assert_eq!(report.status, AccessibilityStatus::Unknown);
        assert_eq!(report.pid, None);
        assert!(report.reason.is_some());
    }
}
