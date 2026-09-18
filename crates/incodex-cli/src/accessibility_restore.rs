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
    fn wait_for_window(&mut self) -> Result<(), String>;
    fn reset(&mut self) -> Result<(), String>;
    fn show_settings_and_app(&mut self) -> Result<(), String>;
    fn wait(&mut self, milliseconds: u64);
}
fn wait_for_decision(ops: &mut impl RestoreOps) -> Result<AccessibilityStatus, String> {
    for _ in 0..120 {
        match ops.probe() {
            status @ (AccessibilityStatus::Granted | AccessibilityStatus::Denied) => {
                return Ok(status)
            }
            AccessibilityStatus::Unknown | AccessibilityStatus::NotRunning => ops.wait(250),
        }
    }
    Err(
        "The restored app's identity or permission remained unavailable; no reset was performed."
            .into(),
    )
}

fn renew(ops: &mut impl RestoreOps) -> Result<Outcome, String> {
    ops.verify_official()?;
    ops.launch()?;
    if wait_for_decision(ops)? == AccessibilityStatus::Granted {
        return Ok(Outcome::Granted);
    }
    // A PID can precede the first Electron window. Let it appear before raising
    // Settings/Finder, and recheck in case access changed during startup.
    ops.wait_for_window()?;
    if wait_for_decision(ops)? == AccessibilityStatus::Granted {
        return Ok(Outcome::Granted);
    }
    // The production caller holds the target transaction lock throughout.
    ops.verify_official()?;
    ops.reset()?;
    ops.show_settings_and_app().map_err(|error| format!(
        "The invalid Accessibility registration was cleared, but the guide could not open. Open System Settings > Privacy & Security > Accessibility, add /Applications/ChatGPT.app, then run incodex doctor. {error}"))?;
    for _ in 0..160 {
        match ops.probe() {
            AccessibilityStatus::Granted => return Ok(Outcome::Granted),
            AccessibilityStatus::Denied
            | AccessibilityStatus::Unknown
            | AccessibilityStatus::NotRunning => ops.wait(750),
        }
    }
    Ok(Outcome::Pending)
}

pub(crate) fn finish_uninstall(root: &std::path::Path, app: &std::path::Path) {
    use incodex_core::{format_kv, format_ok, format_warn};
    println!(
        "{}",
        format_kv(
            "Accessibility",
            "Checking the restored official ChatGPT.",
            None
        )
    );
    let result = (|| {
        let _lock =
            incodex_transaction::acquire_target_lock(root, app, "uninstall-accessibility", None)?;
        renew(&mut SystemOps { app })
    })();
    match result {
        Ok(Outcome::Granted) => println!("{}", format_ok("Official ChatGPT Accessibility access verified.", None)),
        Ok(Outcome::Pending) => println!("{}", format_warn("The app is restored, but Accessibility setup is unfinished. Add the selected ChatGPT in System Settings, then run incodex doctor.", None)),
        Err(error) => println!("{}", format_warn(&format!("The app is restored, but Accessibility could not be renewed: {error}"), None)),
    }
}

struct SystemOps<'a> {
    app: &'a std::path::Path,
}
impl RestoreOps for SystemOps<'_> {
    fn verify_official(&mut self) -> Result<(), String> {
        incodex_macos::verify_original_vendor_bundle(
            self.app,
            Some(incodex_macos::OFFICIAL_BUNDLE_IDENTIFIER),
            None,
            None,
        )
        .map(|_| ())
    }
    fn launch(&mut self) -> Result<(), String> {
        bounded_command(std::process::Command::new("/usr/bin/open").arg(self.app))
    }
    fn probe(&mut self) -> AccessibilityStatus {
        incodex_macos::inspect_accessibility_for_app(self.app).status
    }
    fn wait_for_window(&mut self) -> Result<(), String> {
        let target = incodex_macos::AppQuiescence::for_app(self.app)?;
        for _ in 0..80 {
            if incodex_macos::live_main_window_bounds(target.executable())?.is_some() {
                return Ok(());
            }
            self.wait(250);
        }
        Err("The restored app's window did not appear; no reset was performed. Open ChatGPT and check with incodex doctor.".into())
    }
    fn reset(&mut self) -> Result<(), String> {
        bounded_command(std::process::Command::new("/usr/bin/tccutil").args([
            "reset",
            "Accessibility",
            incodex_macos::OFFICIAL_BUNDLE_IDENTIFIER,
        ]))
    }
    fn show_settings_and_app(&mut self) -> Result<(), String> {
        use incodex_core::format_kv;
        bounded_command(
            std::process::Command::new("/usr/bin/open").arg(
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            ),
        )?;
        bounded_command(
            std::process::Command::new("/usr/bin/open")
                .arg("-R")
                .arg(self.app),
        )?;
        println!("{}", format_kv("Accessibility", "Drag the selected ChatGPT from Finder into the Accessibility list. Complete any macOS authentication. Checking automatically for up to two minutes.", None));
        Ok(())
    }
    fn wait(&mut self, milliseconds: u64) {
        std::thread::sleep(std::time::Duration::from_millis(milliseconds));
    }
}

fn bounded_command(command: &mut std::process::Command) -> Result<(), String> {
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let program = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| format!("{program}: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("{program} exited with {status}")),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {
                    Err(error) => format!("{program}: {error}"),
                    _ => format!("{program} timed out"),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    struct Fake {
        states: VecDeque<AccessibilityStatus>,
        events: Vec<&'static str>,
        reset_error: bool,
        show_error: bool,
        verify_error: bool,
    }
    impl Fake {
        fn new(states: &[AccessibilityStatus]) -> Self {
            Self {
                states: states.iter().copied().collect(),
                events: vec![],
                reset_error: false,
                show_error: false,
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
            if self.show_error {
                Err("open failed".into())
            } else {
                Ok(())
            }
        }
        fn wait_for_window(&mut self) -> Result<(), String> {
            self.events.push("window");
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
        let mut ops = Fake::new(&[AccessibilityStatus::Unknown; 120]);
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
    #[test]
    fn transient_identity_changes_are_retried_before_and_after_reset() {
        let mut ops = Fake::new(&[
            AccessibilityStatus::NotRunning,
            AccessibilityStatus::Unknown,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Unknown,
            AccessibilityStatus::Granted,
        ]);
        assert_eq!(renew(&mut ops), Ok(Outcome::Granted));
        assert_eq!(ops.events.iter().filter(|x| **x == "reset").count(), 1);
    }
    #[test]
    fn waits_for_window_and_rechecks_access_before_resetting() {
        let mut ops = Fake::new(&[AccessibilityStatus::Denied, AccessibilityStatus::Granted]);
        assert_eq!(renew(&mut ops), Ok(Outcome::Granted));
        assert!(ops.events.contains(&"window"));
        assert!(!ops.events.contains(&"reset"));
    }
    #[test]
    fn handoff_failure_explains_that_reset_happened_and_how_to_finish() {
        let mut ops = Fake::new(&[AccessibilityStatus::Denied]);
        ops.show_error = true;
        let error = renew(&mut ops).unwrap_err();
        assert!(error.contains("registration was cleared"));
        assert!(error.contains("System Settings"));
        assert!(error.contains("/Applications/ChatGPT.app"));
    }

    #[test]
    fn denied_access_is_not_reset_before_the_native_guide_can_obtain_allow() {
        // A guide that cannot be presented is not a user Allow.  In particular,
        // its failure must not leave the official registration reset as a side
        // effect.  The current implementation resets before calling `show`,
        // so this is intentionally red until the native-guide handoff is wired.
        let mut ops = Fake::new(&[AccessibilityStatus::Denied]);
        ops.show_error = true;

        let _ = renew(&mut ops);

        assert_eq!(ops.events.iter().filter(|event| **event == "reset").count(), 0);
    }

    #[test]
    fn native_guide_surface_precedes_the_single_allow_reset() {
        // The guide owns the decision.  Reset and System Settings may follow
        // only after its Allow path, never before the surface is ready.
        let mut ops = Fake::new(&[
            AccessibilityStatus::Denied,
            AccessibilityStatus::Denied,
            AccessibilityStatus::Granted,
        ]);

        let _ = renew(&mut ops);

        let guide = ops
            .events
            .iter()
            .position(|event| *event == "show")
            .expect("the native guide surface should be recorded");
        let reset = ops
            .events
            .iter()
            .position(|event| *event == "reset")
            .expect("Allow should be the only path to reset");
        assert!(guide < reset, "reset happened before the Allow-capable guide");
        assert_eq!(ops.events.iter().filter(|event| **event == "reset").count(), 1);
    }
}
