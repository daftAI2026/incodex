// @ts-nocheck
/**
 * [INPUT]: 依赖宿主动作、权限文案代际与构建期device-key激活 token。
 * [OUTPUT]: 提供protocol 1动作及纯权限规则；只接受匹配代际的Mac adapter激活。
 * [POS]: 可热替换主动作工厂；准备期不读文件或加载native module。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
"use strict";

const DEVICE_KEY_HOT_FEATURE = "__INCODEX_DEVICE_KEY_HOT_FEATURE__";
const DEVICE_KEY_ACTIVATION_ACTION = "activate-device-key:__INCODEX_DEVICE_KEY_ACTIVATION_TOKEN__";

// 动作代际只捕获稳定宿主能力；adapter资源只在显式激活动作中读取。
function createMainActions(deps, features: any = undefined) {
  const deviceKeyFeature = features?.deviceKeyAdapter ?? DEVICE_KEY_HOT_FEATURE;
  async function open(sourceWindow) {
    if (deps.isIncognito()) {
      return { ok: false, code: "ALREADY_INCOGNITO", reason: "already-incognito" };
    }
    const result = await deps.launchIncognito(sourceWindow);
    return { ok: result.ok === true,
      code: result.ok ? "OK" : String(result.reason || "FAILED").toUpperCase(), reason: result.reason };
  }
  async function handle(action, payload, sourceWindow) {
    if (action === DEVICE_KEY_ACTIVATION_ACTION) {
      if (deps.isIncognito() || typeof deviceKeyFeature?.activate !== "function") {
        return { ok: false, code: "UNAVAILABLE" };
      }
      return deviceKeyFeature.activate();
    }
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
