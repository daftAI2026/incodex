pub(super) const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(750);
const PRIMARY_CHECKS_REQUIRED: u8 = 3;
const FALLBACK_CONFIRMATION_FAILURES: u8 = 2;
const TOTAL_CHECKS_ALLOWED: u8 = 20;

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
    Pending,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Action {
    Confirmed,
    Wait,
    SelectFallback,
    Unresolved,
}

#[derive(Default)]
pub(crate) struct Readiness {
    fallback_attempted: bool,
    primary_other_checks: u8,
    fallback_confirmation_failures: u8,
    total_checks: u8,
    unresolved: bool,
}

impl Readiness {
    pub(super) fn observe(&mut self, page_state: PageState) -> Action {
        if self.unresolved {
            return Action::Unresolved;
        }
        self.total_checks = self.total_checks.saturating_add(1);
        if page_state == PageState::Codex {
            return Action::Confirmed;
        }
        if self.total_checks >= TOTAL_CHECKS_ALLOWED {
            self.unresolved = true;
            return Action::Unresolved;
        }
        if page_state == PageState::Pending {
            if !self.fallback_attempted {
                self.primary_other_checks = 0;
            }
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
}
