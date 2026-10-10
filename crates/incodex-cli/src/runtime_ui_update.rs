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
    controller: serde_json::Value,
    published: Option<serde_json::Value>,
    renderer_ack_id: Option<String>,
    activation_ack: &'static str,
    selection: &'static str,
    rollback: &'static str,
    failure: Option<&'static str>,
    restart_required: bool,
}

fn identity(generation: &UiGeneration) -> serde_json::Value {
    serde_json::json!({
        "release": generation.release,
        "injectorId": generation.files.get("incodex-inject.js"),
    })
}

impl UiUpdate {
    pub fn new(active: UiGeneration) -> Self {
        Self {
            controller: identity(&active),
            active,
            phase: "active",
            published: None,
            renderer_ack_id: None,
            activation_ack: "not-attempted",
            selection: "unconfirmed",
            rollback: "not-needed",
            failure: None,
            restart_required: false,
        }
    }
    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "phase": self.phase,
            "published": self.published,
            "controller": self.controller,
            "activeUi": identity(&self.active),
            "rendererAckId": self.renderer_ack_id,
            "activationAck": self.activation_ack,
            "selection": self.selection,
            "rollback": self.rollback,
            "failure": self.failure,
            "restartRequired": self.restart_required,
            // Runtime manifests cannot prove whether a newer native helper is available.
            "installRequired": null,
        })
    }
    pub fn active(&self) -> &UiGeneration {
        &self.active
    }
    pub fn phase(&self) -> &'static str {
        self.phase
    }
    pub fn preparation_failed(&mut self) {
        self.published = None;
        self.selection = "unconfirmed";
        self.failure = Some("verification-failed");
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
        self.published = Some(identity(&candidate));
        self.selection = "selected";
        self.activation_ack = "not-attempted";
        self.failure = None;
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
        self.restart_required = !compatible;
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
                self.selection = if result.is_err() {
                    "unconfirmed"
                } else {
                    "superseded"
                };
                self.failure = Some("selection-unconfirmed");
                return Err(result
                    .err()
                    .unwrap_or_else(|| "Runtime candidate superseded".into()));
            }
        }
        self.rollback = "not-needed";
        // An attempted evaluation can run even when its reply is lost.
        self.renderer_ack_id = None;
        self.activation_ack = "unconfirmed";
        self.failure = Some("activation-unconfirmed");
        let activation = apply(&candidate).and_then(|ack| {
            if !ack {
                return Err("Renderer did not acknowledge activation".into());
            }
            self.activation_ack = "accepted";
            self.renderer_ack_id = candidate.files.get("incodex-inject.js").cloned();
            self.failure = Some("selection-unconfirmed");
            match selected(&candidate) {
                Ok(true) => self.selection = "selected",
                result => {
                    self.published = None;
                    self.selection = if result.is_err() {
                        "unconfirmed"
                    } else {
                        "superseded"
                    };
                    return Err(result
                        .err()
                        .unwrap_or_else(|| "Runtime candidate superseded".into()));
                }
            }
            Ok(())
        });
        if let Err(error) = activation {
            self.renderer_ack_id = None;
            self.phase = if apply(&self.active).unwrap_or(false) {
                self.renderer_ack_id = self.active.files.get("incodex-inject.js").cloned();
                self.rollback = "succeeded";
                "retained"
            } else {
                self.rollback = "failed";
                "rollback-failed"
            };
            return Err(error);
        }
        self.active = candidate;
        self.phase = "active";
        self.failure = None;
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

    #[test]
    fn diagnostics_separate_publication_controller_and_real_renderer_ack() {
        let mut update = UiUpdate::new(generation("a"));
        assert!(update.snapshot()["rendererAckId"].is_null());
        update
            .activate(generation("b"), |_| Ok(true), |_| Ok(true))
            .unwrap();
        let state = update.snapshot();
        assert_eq!(state["published"]["release"], "b");
        assert_eq!(state["controller"]["release"], "a");
        assert_eq!(state["activeUi"]["release"], "b");
        assert_eq!(state["rendererAckId"], "b".repeat(64));
        assert_eq!(state["activationAck"], "accepted");
        assert_eq!(state["selection"], "selected");
        assert_eq!(state["restartRequired"], false);
        assert!(state["installRequired"].is_null());
    }

    #[test]
    fn same_phase_and_active_ui_still_report_each_incompatible_publication() {
        let mut update = UiUpdate::new(generation("a"));
        let mut states = Vec::new();
        for name in ["b", "c"] {
            let mut candidate = generation(name);
            candidate
                .files
                .insert("incodex-main.cjs".into(), name.into());
            update
                .activate(candidate, |_| Ok(true), |_| panic!("incompatible"))
                .unwrap();
            states.push(update.snapshot());
        }
        assert_ne!(states[0], states[1]);
        assert_eq!(states[1]["published"]["release"], "c");
        assert_eq!(states[1]["activeUi"]["release"], "a");
        assert_eq!(states[1]["restartRequired"], true);
    }

    #[test]
    fn diagnostics_keep_failed_activation_distinct_from_successful_rollback() {
        let mut update = UiUpdate::new(generation("a"));
        assert!(update
            .activate(generation("b"), |_| Ok(true), |g| Ok(g.release == "a"))
            .is_err());
        let state = update.snapshot();
        assert_eq!(state["published"]["release"], "b");
        assert_eq!(state["activationAck"], "unconfirmed");
        assert_eq!(state["rollback"], "succeeded");
        assert_eq!(state["rendererAckId"], "a".repeat(64));
        assert_eq!(state["failure"], "activation-unconfirmed");
    }

    #[test]
    fn verification_failure_clears_publication_without_hiding_uncertain_rollback() {
        let mut update = UiUpdate::new(generation("a"));
        assert!(update
            .activate(generation("b"), |_| Ok(true), |_| Ok(false))
            .is_err());
        update.preparation_failed();
        let state = update.snapshot();
        assert!(state["published"].is_null());
        assert_eq!(state["failure"], "verification-failed");
        assert_eq!(state["rollback"], "failed");
        assert!(state["rendererAckId"].is_null());
        assert_eq!(state["phase"], "rollback-failed");
    }

    #[test]
    fn diagnostics_record_supersession_after_ack_without_copying_raw_errors() {
        let mut update = UiUpdate::new(generation("a"));
        let mut calls = 0;
        assert!(update
            .activate(
                generation("b"),
                |_| {
                    calls += 1;
                    if calls == 1 {
                        Ok(true)
                    } else {
                        Err("https://account.invalid <DOM> person@example.invalid".into())
                    }
                },
                |_| Ok(true)
            )
            .is_err());
        let state = update.snapshot();
        assert!(state["published"].is_null());
        assert_eq!(state["activationAck"], "accepted");
        assert_eq!(state["selection"], "unconfirmed");
        assert_eq!(state["rollback"], "succeeded");
        assert_eq!(state["failure"], "selection-unconfirmed");
        let serialized = state.to_string();
        for private in ["https://", "<DOM>", "person@example"] {
            assert!(!serialized.contains(private));
        }
    }
}
