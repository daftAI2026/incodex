// @ts-nocheck
/**
 * [INPUT]: 依赖宿主显式传入的动作能力、权限文案代际与纯语言解析器。
 * [OUTPUT]: 提供 protocol 1 动作，并可选提供 protocol 1 权限呈现规则。
 * [POS]: 可热替换的纯业务工厂；权限规则只解释缺失，不持有资源或授权事实。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
"use strict";

// 动作代际只捕获稳定宿主能力。权限工厂只读文案与纯语言解析器，不登记资源。
function createMainActions(deps, features: any = undefined) {
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
  const actions = { protocol: 1, handle, open };
  if (features?.permissionCopy !== undefined) {
    const copy = features.permissionCopy;
    const permissionLocale = deps.permissionLocale;
    if (!copy || typeof copy !== "object" || Array.isArray(copy) ||
        typeof permissionLocale?.resolveLocaleFromCatalog !== "function" ||
        typeof permissionLocale?.resolveLocaleDirection !== "function") {
      throw new Error("[incodex] permission locale capability unavailable");
    }
    const permission = Object.freeze({
      resolveCopy(locale) {
        const raw = typeof locale === "string" ? locale : "";
        const key = permissionLocale.resolveLocaleFromCatalog(raw, copy);
        const selected = copy[key] || copy.en;
        return selected && typeof selected === "object" && !Array.isArray(selected) ? selected : null;
      },
      resolveLayoutDirection(locale) {
        const raw = typeof locale === "string" ? locale : "";
        return permissionLocale.resolveLocaleDirection(raw, copy) === "rightToLeft" ? "rightToLeft" : "leftToRight";
      },
      shouldPrompt(snapshot) {
        return snapshot?.accessibility === "not-granted" || snapshot?.screen === "not-granted";
      },
    });
    return Object.freeze({ ...actions, featureProtocol: 1, permission });
  }
  return Object.freeze(actions);
}

export { createMainActions };
