//! Post-uninstall Accessibility renewal for the restored official app.
//!
//! The shared guide loop lives in [`crate::accessibility_guide_host`].  This
//! wrapper supplies the official-vendor verifier; install uses the same loop
//! with its own already-validated target verifier.

use crate::accessibility_guide_host::{run_permission_guide, GuideCopyContext, Outcome};

pub(crate) fn finish_uninstall(root: &std::path::Path, app: &std::path::Path) {
    use incodex_core::{format_kv, format_ok, format_warn};
    println!(
        "{}",
        format_kv(
            "Accessibility",
            "Checking the restored official ChatGPT.",
            None,
        )
    );
    let result = (|| {
        let _lock =
            incodex_transaction::acquire_target_lock(root, app, "uninstall-accessibility", None)?;
        run_permission_guide(root, app, GuideCopyContext::Official, || {
            incodex_macos::verify_original_vendor_bundle(
                app,
                Some(incodex_macos::OFFICIAL_BUNDLE_IDENTIFIER),
                None,
                None,
            )
            .map(|_| ())
        })
    })();
    match result {
        Ok(Outcome::Granted) => println!(
            "{}",
            format_ok("Official ChatGPT Accessibility access verified.", None)
        ),
        Ok(Outcome::Pending) => println!(
            "{}",
            format_warn(
                "The restored app is waiting for Accessibility approval. Run `incodex accessibility` when ready; `incodex doctor` only checks access.",
                None,
            )
        ),
        Err(error) => println!(
            "{}",
            format_warn(
                &format!("The restored app's Accessibility renewal could not finish: {error}. Run `incodex accessibility` to retry."),
                None,
            )
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::super::accessibility_guide_host::{
        run_permission_guide_with_timeouts, GuideHost, GuideHostFactory, GuideOps, HostEvent,
        HostState, Outcome,
    };
    use incodex_macos::AccessibilityStatus;
    use std::collections::VecDeque;
    use std::path::Path;
    use std::time::Duration;

    struct FakeOps {
        states: VecDeque<AccessibilityStatus>,
        events: Vec<&'static str>,
        reset_error: bool,
        settings_error: bool,
        settings_error_after_first: bool,
        settings_calls: usize,
    }

    impl FakeOps {
        fn new(states: &[AccessibilityStatus]) -> Self {
            Self {
                states: states.iter().copied().collect(),
                events: vec![],
                reset_error: false,
                settings_error: false,
                settings_error_after_first: false,
                settings_calls: 0,
            }
        }
    }

    impl GuideOps for FakeOps {
        fn launch(&mut self) -> Result<(), String> {
            self.events.push("launch");
            Ok(())
        }

        fn probe(&mut self) -> AccessibilityStatus {
            self.events.push("probe");
            self.states
                .pop_front()
                .unwrap_or(AccessibilityStatus::Denied)
        }

        fn wait_for_window(&mut self) -> Result<(), String> {
            self.events.push("window");
            Ok(())
        }

        fn reset(&mut self) -> Result<(), String> {
            self.events.push("reset");
            if self.reset_error {
                Err("reset failed".into())
            } else {
                Ok(())
            }
        }

        fn open_settings(&mut self) -> Result<(), String> {
            self.events.push("settings");
            self.settings_calls += 1;
            if self.settings_error || (self.settings_error_after_first && self.settings_calls > 1) {
                Err("settings failed".into())
            } else {
                Ok(())
            }
        }

        fn wait(&mut self, _: Duration) {
            self.events.push("wait");
        }
    }

    struct FakeHost {
        events: VecDeque<HostEvent>,
        poll_delays: VecDeque<Duration>,
        states: Vec<HostState>,
        closed: bool,
        trace: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl FakeHost {
        fn new(events: &[HostEvent]) -> Self {
            Self {
                events: events.iter().cloned().collect(),
                poll_delays: VecDeque::new(),
                states: vec![],
                closed: false,
                trace: Default::default(),
            }
        }
    }

    impl GuideHost for FakeHost {
        fn send_state(&mut self, state: HostState) -> Result<(), String> {
            self.trace.lock().unwrap().push(format!("state:{state:?}"));
            self.states.push(state);
            Ok(())
        }

        fn poll(&mut self, _: Duration) -> Result<HostEvent, String> {
            if let Some(delay) = self.poll_delays.pop_front() {
                std::thread::sleep(delay);
            }
            let event = self.events.pop_front().unwrap_or(HostEvent::Timeout);
            self.trace.lock().unwrap().push(format!("poll:{event:?}"));
            Ok(event)
        }

        fn close(&mut self) {
            if self.closed {
                return;
            }
            self.closed = true;
            self.trace.lock().unwrap().push("close".into());
        }
    }

    impl Drop for FakeHost {
        fn drop(&mut self) {
            self.close();
        }
    }

    struct FakeFactory {
        host: Option<FakeHost>,
        start_error: Option<String>,
    }

    impl FakeFactory {
        fn new(events: &[HostEvent]) -> Self {
            Self {
                host: Some(FakeHost::new(events)),
                start_error: None,
            }
        }
    }

    impl GuideHostFactory for FakeFactory {
        fn start(&mut self, _: &Path, _: &Path) -> Result<Box<dyn GuideHost>, String> {
            if let Some(error) = self.start_error.take() {
                return Err(error);
            }
            Ok(Box::new(self.host.take().expect("fake host is single-use")))
        }
    }

    fn run_fake<F>(
        ops: &mut FakeOps,
        factory: &mut FakeFactory,
        mut verify: F,
    ) -> Result<Outcome, String>
    where
        F: FnMut() -> Result<(), String>,
    {
        run_fake_with_timeouts(
            ops,
            factory,
            &mut verify,
            Duration::from_secs(1),
            Duration::from_secs(1),
        )
    }

    fn run_fake_with_timeouts<F>(
        ops: &mut FakeOps,
        factory: &mut FakeFactory,
        verify: &mut F,
        choice_timeout: Duration,
        guide_timeout: Duration,
    ) -> Result<Outcome, String>
    where
        F: FnMut() -> Result<(), String>,
    {
        run_permission_guide_with_timeouts(
            ops,
            Path::new("/tmp/incodex-test-root"),
            Path::new("/Applications/ChatGPT.app"),
            verify,
            factory,
            choice_timeout,
            guide_timeout,
        )
    }

    #[test]
    fn an_existing_grant_never_starts_guide_or_resets() {
        let mut ops = FakeOps::new(&[AccessibilityStatus::Granted]);
        let mut factory = FakeFactory::new(&[HostEvent::Ready]);
        let result = run_fake(&mut ops, &mut factory, || Ok(()));
        assert_eq!(result, Ok(Outcome::Granted));
        assert_eq!(ops.events, ["launch", "probe"]);
    }

    #[test]
    fn guide_later_returns_pending_without_reset_or_settings() {
        let mut ops = FakeOps::new(&[AccessibilityStatus::Denied, AccessibilityStatus::Denied]);
        let mut factory = FakeFactory::new(&[HostEvent::Ready, HostEvent::Later]);
        let result = run_fake(&mut ops, &mut factory, || Ok(()));
        assert_eq!(result, Ok(Outcome::Pending));
        assert!(!ops.events.contains(&"reset"));
        assert!(!ops.events.contains(&"settings"));
    }

    #[test]
    fn closing_the_guide_is_pending_and_never_resets() {
        let mut ops = FakeOps::new(&[AccessibilityStatus::Denied, AccessibilityStatus::Denied]);
        let mut factory = FakeFactory::new(&[HostEvent::Ready, HostEvent::Close]);
        assert_eq!(
            run_fake(&mut ops, &mut factory, || Ok(())),
            Ok(Outcome::Pending)
        );
        assert!(!ops.events.contains(&"reset"));
        assert!(!ops.events.contains(&"settings"));
    }

    #[test]
    fn later_after_allow_stays_pending_without_repeating_the_reset() {
        let mut ops = FakeOps::new(&[
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
        ]);
        let mut factory = FakeFactory::new(&[HostEvent::Ready, HostEvent::Allow, HostEvent::Later]);
        assert_eq!(
            run_fake(&mut ops, &mut factory, || Ok(())),
            Ok(Outcome::Pending)
        );
        assert_eq!(
            ops.events.iter().filter(|event| **event == "reset").count(),
            1
        );
        assert_eq!(
            ops.events
                .iter()
                .filter(|event| **event == "settings")
                .count(),
            1
        );
    }

    #[test]
    fn reset_failure_keeps_the_error_page_open_until_the_user_dismisses_it() {
        let mut ops = FakeOps::new(&[
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
        ]);
        ops.reset_error = true;
        let mut factory = FakeFactory::new(&[HostEvent::Ready, HostEvent::Allow, HostEvent::Later]);
        let trace = factory.host.as_ref().unwrap().trace.clone();

        let result = run_fake(&mut ops, &mut factory, || Ok(()));

        assert_eq!(result, Err("reset failed".into()));
        assert_eq!(
            ops.events.iter().filter(|event| **event == "reset").count(),
            1
        );
        assert!(!ops.events.contains(&"settings"));
        let trace = trace.lock().unwrap();
        let error = trace
            .iter()
            .position(|event| event == "state:Error(\"reset failed\")")
            .expect("reset failure must be shown in the native guide");
        let dismissal = trace
            .iter()
            .position(|event| event == "poll:Later")
            .expect("the fake host must receive the user's dismissal");
        let close = trace.iter().position(|event| event == "close").unwrap();
        assert!(
            error < dismissal && dismissal < close,
            "error page must remain open through dismissal: {trace:?}"
        );
    }

    #[test]
    fn settings_launch_failure_keeps_the_error_page_open_until_the_user_dismisses_it() {
        let mut ops = FakeOps::new(&[
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
        ]);
        ops.settings_error = true;
        let mut factory = FakeFactory::new(&[HostEvent::Ready, HostEvent::Allow, HostEvent::Later]);
        let trace = factory.host.as_ref().unwrap().trace.clone();

        let result = run_fake(&mut ops, &mut factory, || Ok(()));

        assert_eq!(result, Err("settings failed".into()));
        assert_eq!(
            ops.events.iter().filter(|event| **event == "reset").count(),
            1
        );
        assert_eq!(
            ops.events
                .iter()
                .filter(|event| **event == "settings")
                .count(),
            1
        );
        let trace = trace.lock().unwrap();
        let error = trace
            .iter()
            .position(|event| event == "state:Error(\"settings failed\")")
            .expect("Settings launch failure must be shown in the native guide");
        let dismissal = trace
            .iter()
            .position(|event| event == "poll:Later")
            .expect("the fake host must receive the user's dismissal");
        let close = trace.iter().position(|event| event == "close").unwrap();
        assert!(
            error < dismissal && dismissal < close,
            "error page must remain open through dismissal: {trace:?}"
        );
    }

    #[test]
    fn retry_settings_launch_failure_keeps_error_open_without_repeating_reset() {
        let mut ops = FakeOps::new(&[
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
        ]);
        ops.settings_error_after_first = true;
        let mut factory = FakeFactory::new(&[
            HostEvent::Ready,
            HostEvent::Allow,
            HostEvent::Retry,
            HostEvent::Later,
        ]);
        let trace = factory.host.as_ref().unwrap().trace.clone();

        let result = run_fake(&mut ops, &mut factory, || Ok(()));

        assert_eq!(result, Err("settings failed".into()));
        assert_eq!(
            ops.events.iter().filter(|event| **event == "reset").count(),
            1
        );
        assert_eq!(
            ops.settings_calls, 2,
            "initial open should succeed and Retry should fail"
        );
        let trace = trace.lock().unwrap();
        let error = trace
            .iter()
            .position(|event| event == "state:Error(\"settings failed\")")
            .expect("retry Settings failure must be shown in the native guide");
        let dismissal = trace
            .iter()
            .position(|event| event == "poll:Later")
            .expect("the fake host must receive the user's dismissal");
        let close = trace.iter().position(|event| event == "close").unwrap();
        assert!(
            error < dismissal && dismissal < close,
            "retry error page must remain open through dismissal: {trace:?}"
        );
    }

    #[test]
    fn post_allow_timeout_keeps_the_error_page_open_until_the_user_dismisses_it() {
        let mut ops = FakeOps::new(&[
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
        ]);
        let mut factory = FakeFactory::new(&[
            HostEvent::Ready,
            HostEvent::Allow,
            HostEvent::Timeout,
            HostEvent::Later,
        ]);
        factory.host.as_mut().unwrap().poll_delays.extend([
            Duration::ZERO,
            Duration::ZERO,
            Duration::from_millis(30),
        ]);
        let trace = factory.host.as_ref().unwrap().trace.clone();
        let mut verify = || Ok(());

        let result = run_fake_with_timeouts(
            &mut ops,
            &mut factory,
            &mut verify,
            Duration::from_secs(1),
            Duration::from_millis(5),
        );

        assert_eq!(result, Ok(Outcome::Pending));
        assert_eq!(
            ops.events.iter().filter(|event| **event == "reset").count(),
            1
        );
        assert_eq!(
            ops.events
                .iter()
                .filter(|event| **event == "settings")
                .count(),
            1
        );
        let trace = trace.lock().unwrap();
        let error = trace
            .iter()
            .position(|event| event.contains("state:Error(\"native Accessibility guide timed out"))
            .expect("guide timeout must be shown in the native guide");
        let dismissal = trace
            .iter()
            .rposition(|event| event == "poll:Later")
            .expect("the fake host must receive the user's dismissal after timeout");
        let close = trace.iter().position(|event| event == "close").unwrap();
        assert!(
            error < dismissal && dismissal < close,
            "timeout page must remain open through dismissal: {trace:?}"
        );
    }

    #[test]
    fn choice_wait_has_its_own_bounded_deadline_before_allow() {
        let mut ops = FakeOps::new(&[
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Granted,
        ]);
        let mut factory = FakeFactory::new(&[HostEvent::Ready, HostEvent::Allow]);
        factory
            .host
            .as_mut()
            .unwrap()
            .poll_delays
            .extend([Duration::ZERO, Duration::from_millis(50)]);
        let mut verify = || Ok(());
        let result = run_fake_with_timeouts(
            &mut ops,
            &mut factory,
            &mut verify,
            Duration::from_millis(20),
            Duration::from_millis(100),
        );
        assert_eq!(result, Ok(Outcome::Pending));
        assert!(!ops.events.contains(&"reset"));
        assert!(!ops.events.contains(&"settings"));
    }

    #[test]
    fn a_host_error_after_ready_is_visible_and_never_resets() {
        let mut ops = FakeOps::new(&[AccessibilityStatus::Denied, AccessibilityStatus::Denied]);
        let mut factory = FakeFactory::new(&[
            HostEvent::Ready,
            HostEvent::Error("native view failed".into()),
        ]);
        let result = run_fake(&mut ops, &mut factory, || Ok(()));
        assert_eq!(
            result,
            Err("native Accessibility guide reported an error: native view failed".into())
        );
        assert!(!ops.events.contains(&"reset"));
    }

    #[test]
    fn a_host_error_before_ready_is_visible_and_never_resets() {
        let mut ops = FakeOps::new(&[AccessibilityStatus::Denied, AccessibilityStatus::Denied]);
        let mut factory = FakeFactory::new(&[HostEvent::Error("native startup failed".into())]);
        let result = run_fake(&mut ops, &mut factory, || Ok(()));
        assert_eq!(
            result,
            Err(
                "native Accessibility guide failed before becoming ready: native startup failed"
                    .into()
            )
        );
        assert!(!ops.events.contains(&"reset"));
    }

    #[test]
    fn allow_revalidates_confirmed_denial_and_resets_once() {
        let mut ops = FakeOps::new(&[
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Granted,
        ]);
        let mut factory = FakeFactory::new(&[HostEvent::Ready, HostEvent::Allow]);
        let mut verify_calls = 0;
        let result = run_fake(&mut ops, &mut factory, || {
            verify_calls += 1;
            Ok(())
        });
        assert_eq!(result, Ok(Outcome::Granted));
        assert_eq!(verify_calls, 2);
        assert_eq!(
            ops.events.iter().filter(|event| **event == "reset").count(),
            1
        );
        assert_eq!(
            ops.events
                .iter()
                .filter(|event| **event == "settings")
                .count(),
            1
        );
    }

    #[test]
    fn retry_reopens_settings_without_a_second_reset() {
        let mut ops = FakeOps::new(&[
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Granted,
        ]);
        let mut factory = FakeFactory::new(&[HostEvent::Ready, HostEvent::Allow, HostEvent::Retry]);
        let result = run_fake(&mut ops, &mut factory, || Ok(()));
        assert_eq!(result, Ok(Outcome::Granted));
        assert_eq!(
            ops.events.iter().filter(|event| **event == "reset").count(),
            1
        );
        assert_eq!(
            ops.events
                .iter()
                .filter(|event| **event == "settings")
                .count(),
            2
        );
    }

    #[test]
    fn shared_controller_automatically_closes_after_external_grant_without_retry() {
        let mut ops = FakeOps::new(&[
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Granted,
        ]);
        let mut factory =
            FakeFactory::new(&[HostEvent::Ready, HostEvent::Allow, HostEvent::Timeout]);
        let trace = factory.host.as_ref().unwrap().trace.clone();
        assert_eq!(
            run_fake(&mut ops, &mut factory, || Ok(())),
            Ok(Outcome::Granted)
        );
        let trace = trace.lock().unwrap();
        assert_eq!(&trace[trace.len() - 2..], &["state:Granted", "close"]);
        assert_eq!(ops.events.iter().filter(|e| **e == "reset").count(), 1);
        assert_eq!(ops.events.iter().filter(|e| **e == "settings").count(), 1);
    }

    #[test]
    fn unknown_status_after_allow_is_pending_and_never_resets() {
        let mut ops = FakeOps::new(&[
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Unknown,
        ]);
        let mut factory = FakeFactory::new(&[HostEvent::Ready, HostEvent::Allow]);
        let result = run_fake(&mut ops, &mut factory, || Ok(()));
        assert_eq!(result, Ok(Outcome::Pending));
        assert!(!ops.events.contains(&"reset"));
    }

    #[test]
    fn target_verifier_failure_before_launch_prevents_guide_and_reset() {
        let mut ops = FakeOps::new(&[AccessibilityStatus::Denied]);
        let mut factory = FakeFactory::new(&[HostEvent::Ready, HostEvent::Allow]);
        let result = run_fake(&mut ops, &mut factory, || Err("target changed".into()));
        assert_eq!(result, Err("target changed".into()));
        assert!(ops.events.is_empty());
    }

    #[test]
    fn target_verifier_failure_before_allow_prevents_reset() {
        let mut ops = FakeOps::new(&[AccessibilityStatus::Denied, AccessibilityStatus::Denied]);
        let mut factory = FakeFactory::new(&[HostEvent::Ready, HostEvent::Allow]);
        let mut calls = 0;
        let result = run_fake(&mut ops, &mut factory, || {
            calls += 1;
            if calls == 1 {
                Ok(())
            } else {
                Err("target changed".into())
            }
        });
        assert_eq!(result, Err("target changed".into()));
        assert!(!ops.events.contains(&"reset"));
    }

    #[test]
    fn a_host_start_failure_is_visible_and_never_resets() {
        let mut ops = FakeOps::new(&[AccessibilityStatus::Denied, AccessibilityStatus::Denied]);
        let mut factory = FakeFactory::new(&[]);
        factory.start_error = Some("host unavailable".into());
        let result = run_fake(&mut ops, &mut factory, || Ok(()));
        assert_eq!(
            result,
            Err("native Accessibility guide could not start: host unavailable".into())
        );
        assert!(!ops.events.contains(&"reset"));
    }
}
