// @ts-nocheck
"use strict";

// No events or resources belong to a generation. Stable dependencies retain
// authorization, native menus, launch single-flight and session cleanup.
function createMainActions(deps) {
  async function open(sourceWindow) {
    if (deps.isIncognito()) {
      return { ok: false, code: "ALREADY_INCOGNITO", reason: "already-incognito" };
    }
    const result = await deps.launchIncognito(sourceWindow);
    return { ok: result.ok === true,
      code: result.ok ? "OK" : String(result.reason || "FAILED").toUpperCase(), reason: result.reason };
  }
  async function handle(action, payload, sourceWindow) {
    if (action === "configure-dock-menu" || action === "configure-status-menu") {
      const configured = action === "configure-dock-menu"
        ? deps.configureDockMenu(payload?.label) === true
        : (await deps.configureStatusMenu(payload?.label)) === true;
      return { ok: configured, code: configured ? "OK" : "UNAVAILABLE" };
    }
    if (action === "open") return open(sourceWindow);
    if (action === "quit") {
      if (!deps.isIncognito()) return { ok: false, code: "NOT_INCOGNITO", reason: "not-incognito" };
      deps.quit();
      return { ok: true, code: "OK" };
    }
    return { ok: false, code: "UNKNOWN_ACTION" };
  }
  return Object.freeze({ protocol: 1, handle, open });
}

export { createMainActions };
