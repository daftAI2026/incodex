// @ts-nocheck
/**
 * [INPUT]: 依赖主 Runtime 的稳定 IO 能力与权限策略工厂。
 * [OUTPUT]: 提供同代动作和权限能力；工厂不执行 IO。
 * [POS]: 可热替换的主服务边界；保留旧动作协议，不持有窗口所有权。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
"use strict";

const { createPermissionProbePolicy } = require("./incodex-permission-policy.cts");

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
  return Object.freeze({ protocol: 1, servicesProtocol: 1, handle, open,
    permissionProbe: createPermissionProbePolicy() });
}

export { createMainActions };
