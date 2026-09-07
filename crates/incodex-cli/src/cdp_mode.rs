use std::time::{Duration, Instant};

pub(crate) const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(750);
pub(super) const ACTIVE_TIMEOUT: Duration = Duration::from_secs(90);
const PRIMARY_CHECKS_REQUIRED: u8 = 3;
const FALLBACK_CONFIRMATION_FAILURES: u8 = 2;
const PROBE_FAILURES_ALLOWED: u8 = 20;

pub(super) const PROBE_EXPRESSION: &str = r#"(() => {
  function visible(element) {
    if (!(element instanceof HTMLElement)) return false;
    if (element.matches(":disabled, [aria-disabled=\"true\"]")) return false;
    if (element.closest('[aria-hidden="true"], [inert]')) return false;
    for (let current = element; current instanceof HTMLElement; current = current.parentElement) {
      const style = getComputedStyle(current);
      if (
        style.display === "none" ||
        style.visibility === "hidden" ||
        style.visibility === "collapse" ||
        Number.parseFloat(style.opacity || "1") <= 0
      ) return false;
    }
    return Array.from(element.getClientRects()).some((rect) =>
      rect.width > 0 &&
      rect.height > 0 &&
      rect.bottom > 0 &&
      rect.right > 0 &&
      rect.top < window.innerHeight &&
      rect.left < window.innerWidth
    );
  }

  const modeButtons = Array.from(document.querySelectorAll(
    'button[aria-haspopup="menu"], [role="button"][aria-haspopup="menu"]',
  )).filter(visible);
  let modeLabel = "";
  for (const button of modeButtons) {
    const text = [button.textContent, button.getAttribute("aria-label")]
      .filter((value) => typeof value === "string")
      .join(" ")
      .replace(/\s+/g, " ")
      .trim();
    if (/\bCodex\b/i.test(text)) {
      modeLabel = "Codex";
      break;
    }
    if (/\bChatGPT\b/i.test(text)) {
      modeLabel = "ChatGPT";
      break;
    }
  }

  const officialBlockerVisible = Array.from(
    document.querySelectorAll(
      'dialog[open], [role="dialog"], [role="alertdialog"], [aria-modal="true"]',
    ),
  ).some(visible);
  return {
    modeAvailable: modeLabel.length > 0,
    modeLabel,
    officialBlockerVisible,
  };
})()"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PageState {
    Codex,
    BlockedByOfficialUi,
    NotReady,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Action {
    Confirmed,
    BlockedByOfficialUi,
    Wait,
    SelectFallback,
    Unresolved,
}

pub(crate) struct Readiness {
    active_timeout: Duration,
    blocked_at: Option<Instant>,
    fallback_attempted: bool,
    paused: Duration,
    primary_other_checks: u8,
    fallback_confirmation_failures: u8,
    probe_failures: u8,
    started_at: Instant,
    unresolved: bool,
}

impl Default for Readiness {
    fn default() -> Self {
        Self::new(Instant::now(), ACTIVE_TIMEOUT)
    }
}

impl Readiness {
    pub(super) fn new(started_at: Instant, active_timeout: Duration) -> Self {
        Self {
            active_timeout,
            blocked_at: None,
            fallback_attempted: false,
            paused: Duration::ZERO,
            primary_other_checks: 0,
            fallback_confirmation_failures: 0,
            probe_failures: 0,
            started_at,
            unresolved: false,
        }
    }

    pub(super) fn observe(&mut self, page_state: PageState) -> Action {
        self.observe_at(page_state, Instant::now())
    }

    pub(super) fn observe_at(&mut self, page_state: PageState, now: Instant) -> Action {
        if self.unresolved {
            return Action::Unresolved;
        }
        self.probe_failures = 0;
        if page_state == PageState::Codex {
            self.resume_active_clock(now);
            return Action::Confirmed;
        }
        if page_state == PageState::BlockedByOfficialUi {
            if !self.fallback_attempted {
                self.primary_other_checks = 0;
            }
            self.fallback_confirmation_failures = 0;
            self.blocked_at.get_or_insert(now);
            return Action::BlockedByOfficialUi;
        }
        self.resume_active_clock(now);
        if self.active_elapsed_at(now) >= self.active_timeout {
            self.unresolved = true;
            return Action::Unresolved;
        }
        if page_state == PageState::NotReady {
            if !self.fallback_attempted {
                self.primary_other_checks = 0;
            }
            self.fallback_confirmation_failures = 0;
            return Action::Wait;
        }
        if !self.fallback_attempted {
            self.primary_other_checks += 1;
            if self.primary_other_checks < PRIMARY_CHECKS_REQUIRED {
                return Action::Wait;
            }
            self.fallback_attempted = true;
            return Action::SelectFallback;
        }

        self.fallback_confirmation_failures += 1;
        if self.fallback_confirmation_failures >= FALLBACK_CONFIRMATION_FAILURES {
            self.unresolved = true;
            Action::Unresolved
        } else {
            Action::Wait
        }
    }

    pub(super) fn observe_probe_failure(&mut self) -> Action {
        self.observe_probe_failure_at(Instant::now())
    }

    pub(super) fn observe_probe_failure_at(&mut self, now: Instant) -> Action {
        if self.unresolved {
            return Action::Unresolved;
        }
        self.probe_failures = self.probe_failures.saturating_add(1);
        if self.probe_failures >= PROBE_FAILURES_ALLOWED
            || self.active_elapsed_at(now) >= self.active_timeout
        {
            self.unresolved = true;
            Action::Unresolved
        } else {
            Action::Wait
        }
    }

    fn active_elapsed_at(&self, now: Instant) -> Duration {
        let total = now.saturating_duration_since(self.started_at);
        let current_pause = self
            .blocked_at
            .map(|blocked_at| now.saturating_duration_since(blocked_at))
            .unwrap_or_default();
        total.saturating_sub(self.paused.saturating_add(current_pause))
    }

    fn resume_active_clock(&mut self, now: Instant) {
        if let Some(blocked_at) = self.blocked_at.take() {
            self.paused = self
                .paused
                .saturating_add(now.saturating_duration_since(blocked_at));
        }
    }
}
