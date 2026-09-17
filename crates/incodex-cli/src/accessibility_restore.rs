//! Post-uninstall renewal for the restored official app; never grants TCC access.
use incodex_macos::AccessibilityStatus;

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Granted,
    Pending,
}
trait RestoreOps {
    fn verify_official(&mut self) -> Result<(), String>;
    fn launch(&mut self) -> Result<(), String>;
    fn probe(&mut self) -> AccessibilityStatus;
    fn reset(&mut self) -> Result<(), String>;
    fn show_settings_and_app(&mut self) -> Result<(), String>;
    fn wait(&mut self, milliseconds: u64);
}
fn renew(_ops: &mut impl RestoreOps) -> Result<Outcome, String> {
    Ok(Outcome::Pending)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    struct Fake {
        states: VecDeque<AccessibilityStatus>,
        events: Vec<&'static str>,
        reset_error: bool,
        verify_error: bool,
    }
    impl Fake {
        fn new(states: &[AccessibilityStatus]) -> Self {
            Self {
                states: states.iter().copied().collect(),
                events: vec![],
                reset_error: false,
                verify_error: false,
            }
        }
    }
    impl RestoreOps for Fake {
        fn verify_official(&mut self) -> Result<(), String> {
            self.events.push("verify");
            if self.verify_error {
                Err("not official".into())
            } else {
                Ok(())
            }
        }
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
        fn reset(&mut self) -> Result<(), String> {
            self.events.push("reset");
            if self.reset_error {
                Err("reset failed".into())
            } else {
                Ok(())
            }
        }
        fn show_settings_and_app(&mut self) -> Result<(), String> {
            self.events.push("show");
            Ok(())
        }
        fn wait(&mut self, _: u64) {
            self.events.push("wait");
        }
    }
    #[test]
    fn a_valid_official_grant_is_preserved_without_reset_or_settings() {
        let mut ops = Fake::new(&[AccessibilityStatus::Granted]);
        assert_eq!(renew(&mut ops), Ok(Outcome::Granted));
        assert_eq!(ops.events, ["verify", "launch", "probe"]);
    }
    #[test]
    fn stale_grant_is_reset_once_and_only_a_positive_host_probe_completes() {
        let mut ops = Fake::new(&[
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Granted,
        ]);
        assert_eq!(renew(&mut ops), Ok(Outcome::Granted));
        assert_eq!(ops.events.iter().filter(|x| **x == "reset").count(), 1);
        assert!(ops
            .events
            .windows(3)
            .any(|x| x == ["verify", "reset", "show"]));
        assert!(
            ops.events.iter().position(|x| *x == "show").unwrap()
                < ops.events.iter().rposition(|x| *x == "probe").unwrap()
        );
    }
    #[test]
    fn unknown_host_identity_never_resets() {
        let mut ops = Fake::new(&[AccessibilityStatus::Unknown]);
        assert!(renew(&mut ops).is_err());
        assert!(!ops.events.contains(&"reset"));
    }
    #[test]
    fn changed_bundle_never_launches_or_resets() {
        let mut ops = Fake::new(&[]);
        ops.verify_error = true;
        assert!(renew(&mut ops).is_err());
        assert_eq!(ops.events, ["verify"]);
    }
    #[test]
    fn failed_reset_does_not_open_a_misleading_ready_surface() {
        let mut ops = Fake::new(&[AccessibilityStatus::Denied]);
        ops.reset_error = true;
        assert_eq!(renew(&mut ops), Err("reset failed".into()));
        assert!(!ops.events.contains(&"show"));
    }
    #[test]
    fn still_denied_after_bounded_wait_is_pending_not_granted() {
        let mut ops = Fake::new(&[AccessibilityStatus::Denied]);
        assert_eq!(renew(&mut ops), Ok(Outcome::Pending));
        assert_eq!(ops.events.iter().filter(|x| **x == "reset").count(), 1);
        assert!(ops.events.iter().filter(|x| **x == "wait").count() <= 200);
    }
    #[test]
    fn startup_waits_for_the_restored_process_before_deciding() {
        let mut ops = Fake::new(&[
            AccessibilityStatus::NotRunning,
            AccessibilityStatus::Granted,
        ]);
        assert_eq!(renew(&mut ops), Ok(Outcome::Granted));
        assert!(ops.events.contains(&"wait"));
        assert!(!ops.events.contains(&"reset"));
    }
}
