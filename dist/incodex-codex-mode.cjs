// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.CODEX_MODE_PROBE_EXPRESSION = void 0;
exports.createCodexModeReadiness = createCodexModeReadiness;
exports.decideCodexModeAction = decideCodexModeAction;
exports.deriveCodexModePageState = deriveCodexModePageState;
const CODEX_MODE_PROBE_EXPRESSION = `(() => {
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
      .replace(/\\s+/g, " ")
      .trim();
    if (/\\bCodex\\b/i.test(text)) {
      modeLabel = "Codex";
      break;
    }
    if (/\\bChatGPT\\b/i.test(text)) {
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
})()`;
exports.CODEX_MODE_PROBE_EXPRESSION = CODEX_MODE_PROBE_EXPRESSION;
function deriveCodexModePageState(snapshot) {
    if (snapshot?.modeAvailable === true && snapshot.modeLabel === "Codex")
        return "codex";
    if (snapshot?.officialBlockerVisible === true || snapshot?.modeAvailable !== true) {
        return "pending";
    }
    return "other";
}
function decideCodexModeAction(pageState, fallbackAttempted, confirmationFailures, primaryOtherChecks = 0, primaryOtherChecksRequired = 3) {
    if (pageState === "codex")
        return "confirmed";
    if (pageState === "pending")
        return "wait";
    if (!fallbackAttempted) {
        return primaryOtherChecks >= primaryOtherChecksRequired ? "select-fallback" : "wait";
    }
    if (confirmationFailures >= 2)
        return "unresolved";
    return "wait";
}
function createCodexModeReadiness(options) {
    const checks = new WeakMap();
    const primarySettleMs = options.primarySettleMs ?? 1_500;
    const primaryOtherChecksRequired = options.primaryOtherChecksRequired ?? 3;
    const pollMs = options.pollMs ?? 750;
    const scheduleTimer = options.scheduleTimer ?? setTimeout;
    const cancelTimer = options.cancelTimer ?? clearTimeout;
    function stateFor(win) {
        let state = checks.get(win);
        if (state)
            return state;
        state = {
            complete: false,
            confirmationFailures: 0,
            fallbackAttempted: false,
            fallbackSucceeded: false,
            primaryOtherChecks: 0,
            running: false,
            timer: null,
        };
        checks.set(win, state);
        win.once("closed", () => {
            state.complete = true;
            if (state.timer)
                cancelTimer(state.timer);
        });
        return state;
    }
    function observe(win, delay = primarySettleMs) {
        if (!options.isIncognito() || !win || win.isDestroyed())
            return;
        const state = stateFor(win);
        if (state.complete || state.running || state.timer)
            return;
        state.timer = scheduleTimer(() => {
            state.timer = null;
            void reconcile(win, state);
        }, delay);
    }
    async function reconcile(win, state) {
        if (state.complete || win.isDestroyed() || win.webContents.isDestroyed())
            return;
        state.running = true;
        try {
            const snapshot = await win.webContents.executeJavaScript(CODEX_MODE_PROBE_EXPRESSION, false);
            if (state.complete || win.isDestroyed() || win.webContents.isDestroyed())
                return;
            const pageState = deriveCodexModePageState(snapshot);
            if (!state.fallbackAttempted) {
                state.primaryOtherChecks = pageState === "other" ? state.primaryOtherChecks + 1 : 0;
            }
            else if (pageState === "other") {
                state.confirmationFailures += 1;
            }
            const action = decideCodexModeAction(pageState, state.fallbackAttempted, state.confirmationFailures, state.primaryOtherChecks, primaryOtherChecksRequired);
            if (action === "confirmed") {
                state.complete = true;
                options.log("codex-mode-confirmed", { fallback: state.fallbackSucceeded });
                return;
            }
            if (action === "unresolved") {
                state.complete = true;
                options.log("codex-mode-unresolved", { fallback: state.fallbackSucceeded });
                return;
            }
            if (action === "select-fallback" && win.isFocused()) {
                if (state.complete || win.isDestroyed() || win.webContents.isDestroyed())
                    return;
                state.fallbackAttempted = true;
                state.confirmationFailures = 0;
                state.fallbackSucceeded = options.selectFallback(win) === true;
                if (state.fallbackSucceeded) {
                    options.log("codex-mode-fallback-sent");
                }
                else {
                    state.complete = true;
                    options.log("codex-mode-unresolved", { fallback: false });
                }
            }
        }
        catch (error) {
            options.log("codex-mode-probe-failed", { error: String(error) });
        }
        finally {
            state.running = false;
            if (!state.complete)
                observe(win, pollMs);
        }
    }
    return { observe };
}
