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
    const style = getComputedStyle(element);
    if (style.display === "none" || style.visibility === "hidden") return false;
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  }

  const modeButtons = Array.from(document.querySelectorAll('button[aria-haspopup="menu"]'))
    .filter(visible);
  let modeLabel = "";
  for (const button of modeButtons) {
    const label = Array.from(button.children)
      .find((child) => child.tagName === "SPAN")
      ?.textContent?.trim();
    if (label === "Codex" || label === "ChatGPT") {
      modeLabel = label;
      break;
    }
  }

  const officialBlockerVisible = Array.from(
    document.querySelectorAll('[role="dialog"], [aria-modal="true"]'),
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
function decideCodexModeAction(pageState, fallbackSent, confirmationFailures) {
    if (pageState === "codex")
        return "confirmed";
    if (pageState === "pending")
        return "wait";
    if (!fallbackSent)
        return "select-fallback";
    if (confirmationFailures >= 2)
        return "unresolved";
    return "wait";
}
function createCodexModeReadiness(options) {
    const checks = new WeakMap();
    const primarySettleMs = options.primarySettleMs ?? 1_500;
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
            fallbackSent: false,
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
            const pageState = deriveCodexModePageState(snapshot);
            if (state.fallbackSent && pageState === "other")
                state.confirmationFailures += 1;
            const action = decideCodexModeAction(pageState, state.fallbackSent, state.confirmationFailures);
            if (action === "confirmed") {
                state.complete = true;
                options.log("codex-mode-confirmed", { fallback: state.fallbackSent });
                return;
            }
            if (action === "unresolved") {
                state.complete = true;
                options.log("codex-mode-unresolved", { fallback: state.fallbackSent });
                return;
            }
            if (action === "select-fallback" && win.isFocused()) {
                state.fallbackSent = options.selectFallback(win);
                state.confirmationFailures = 0;
                if (state.fallbackSent)
                    options.log("codex-mode-fallback-sent");
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
