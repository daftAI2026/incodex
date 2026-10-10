//! Generation switching for the single installed Windows primary page.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UiGeneration {
    pub release: String,
    pub source: String,
    pub files: BTreeMap<String, String>,
}

pub(crate) struct UiUpdate {
    active: UiGeneration,
    phase: &'static str,
}

impl UiUpdate {
    pub fn new(active: UiGeneration) -> Self {
        Self {
            active,
            phase: "active",
        }
    }
    pub fn active(&self) -> &UiGeneration {
        &self.active
    }
    pub fn phase(&self) -> &'static str {
        self.phase
    }
    pub fn preparation_failed(&mut self) {
        if self.phase != "rollback-failed" {
            self.phase = "retained";
        }
    }
    pub fn activate(
        &mut self,
        candidate: UiGeneration,
        mut selected: impl FnMut(&UiGeneration) -> Result<bool, String>,
        mut apply: impl FnMut(&UiGeneration) -> Result<bool, String>,
    ) -> Result<(), String> {
        let compatible = self.active.files.contains_key("incodex-inject.js")
            && candidate.files.contains_key("incodex-inject.js")
            && self
                .active
                .files
                .iter()
                .filter(|(name, _)| name.as_str() != "incodex-inject.js")
                .eq(candidate
                    .files
                    .iter()
                    .filter(|(name, _)| name.as_str() != "incodex-inject.js"));
        if !compatible {
            if self.phase != "rollback-failed" {
                self.phase = "restart-required";
            }
            return Ok(());
        }
        if self.active.release == candidate.release && self.phase == "active" {
            return Ok(());
        }
        match selected(&candidate) {
            Ok(true) => {}
            result => {
                self.preparation_failed();
                return Err(result
                    .err()
                    .unwrap_or_else(|| "Runtime candidate superseded".into()));
            }
        }
        let activation = apply(&candidate).and_then(|ack| {
            if !ack {
                return Err("Renderer did not acknowledge activation".into());
            }
            if !selected(&candidate)? {
                return Err("Runtime candidate superseded".into());
            }
            Ok(())
        });
        if let Err(error) = activation {
            self.phase = if apply(&self.active).unwrap_or(false) {
                "retained"
            } else {
                "rollback-failed"
            };
            return Err(error);
        }
        self.active = candidate;
        self.phase = "active";
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn generation(name: &str) -> UiGeneration {
        UiGeneration {
            release: name.into(),
            source: name.into(),
            files: BTreeMap::from([
                ("incodex-inject.js".into(), name.repeat(64)),
                ("incodex-main.cjs".into(), "same".into()),
            ]),
        }
    }

    #[test]
    fn commits_only_a_selected_acknowledged_ui_and_navigation_uses_that_generation() {
        let mut update = UiUpdate::new(generation("a"));
        let mut applied = Vec::new();
        update
            .activate(
                generation("b"),
                |_| Ok(true),
                |value| {
                    applied.push(value.release.clone());
                    Ok(true)
                },
            )
            .unwrap();
        assert_eq!(applied, ["b"]);
        assert_eq!(update.active().source, "b");
        assert_eq!(update.phase(), "active");
    }

    #[test]
    fn lost_ack_rolls_back_even_if_the_candidate_ran_and_retains_previous_source() {
        let mut update = UiUpdate::new(generation("a"));
        let mut applied = Vec::new();
        assert!(update
            .activate(
                generation("b"),
                |_| Ok(true),
                |value| {
                    applied.push(value.release.clone());
                    Ok(value.release == "a")
                }
            )
            .is_err());
        assert_eq!(applied, ["b", "a"]);
        assert_eq!(update.active().source, "a");
        assert_eq!(update.phase(), "retained");
    }

    #[test]
    fn superseded_publication_after_ack_rolls_back_and_keeps_selection_uncommitted() {
        let mut update = UiUpdate::new(generation("a"));
        let mut checks = 0;
        let mut applied = Vec::new();
        assert!(update
            .activate(
                generation("b"),
                |_| {
                    checks += 1;
                    Ok(checks == 1)
                },
                |value| {
                    applied.push(value.release.clone());
                    Ok(true)
                }
            )
            .is_err());
        assert_eq!(applied, ["b", "a"]);
        assert_eq!(update.active().release, "a");
    }

    #[test]
    fn incompatible_assets_and_missing_injector_never_execute() {
        for change in [
            "incodex-main.cjs",
            "incodex-windows-platform.cjs",
            "remove-injector",
        ] {
            let mut update = UiUpdate::new(generation("a"));
            let mut candidate = generation("b");
            if change == "remove-injector" {
                candidate.files.remove("incodex-inject.js");
            } else {
                candidate.files.insert(change.into(), "changed".into());
            }
            update
                .activate(
                    candidate,
                    |_| Ok(true),
                    |_| panic!("incompatible code must not run"),
                )
                .unwrap();
            assert_eq!(update.phase(), "restart-required");
            assert_eq!(update.active().release, "a");
        }
    }

    #[test]
    fn rollback_failure_stays_visible_across_a_later_preparation_error() {
        let mut update = UiUpdate::new(generation("a"));
        assert!(update
            .activate(generation("b"), |_| Ok(true), |_| Ok(false))
            .is_err());
        assert_eq!(update.phase(), "rollback-failed");
        update.preparation_failed();
        assert_eq!(update.phase(), "rollback-failed");
        assert_eq!(update.active().release, "a");
    }

    #[test]
    fn an_unselected_candidate_does_not_activate_or_require_rollback() {
        let mut update = UiUpdate::new(generation("a"));
        assert!(update
            .activate(
                generation("b"),
                |_| Ok(false),
                |_| panic!("superseded code must not run")
            )
            .is_err());
        assert_eq!(update.active().release, "a");
    }
}
