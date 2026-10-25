/**
 * [INPUT]: 依赖共享 Window IPC、菜单文案与平台状态。
 * [OUTPUT]: 提供动作请求、原生菜单及快捷键标签入口。
 * [POS]: injector 与宿主动作之间的薄桥；保留请求和重试语义。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import type { IncognitoAction, IncognitoActionResponse } from "./renderer-bridge-types.ts";

type RendererActionScope = Pick<
  Window,
  | "incodex"
  | "__incodexIncognito"
  | "__incodexPlatform"
  | "__incodexDockMenuConfigured"
  | "__incodexStatusMenuConfigured"
>;
const SHORTCUT_LABEL = "⇧⌘N";

function newRequestId(): string {
  return `incodex-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export function createRendererActionBridge(
  scope: RendererActionScope,
  translateMenuLabel: (key: "title" | "open") => string,
) {
  function isIncognitoWindow(): boolean {
    if (typeof scope.__incodexIncognito === "boolean") return scope.__incodexIncognito;
    return false;
  }

  function isWindowsRenderer(): boolean {
    return scope.__incodexPlatform === "win32";
  }

  function shortcutLabel(): string {
    return isWindowsRenderer() ? "Ctrl+Shift+N" : SHORTCUT_LABEL;
  }

  async function requestAction(action: IncognitoAction): Promise<IncognitoActionResponse> {
    if (!scope.incodex?.requestIncognitoAction) {
      return { ok: false, reason: "unavailable", code: "UNAVAILABLE" };
    }
    try {
      return (
        (await scope.incodex.requestIncognitoAction({ action, requestId: newRequestId() })) ?? {
          ok: false,
          reason: "unavailable",
          code: "UNAVAILABLE",
        }
      );
    } catch {
      return { ok: false, reason: "ipc-failed", code: "IPC_FAILED" };
    }
  }

  function requestDeviceKeyActivation(): Promise<IncognitoActionResponse> {
    return scope.incodex?.requestIncognitoAction?.({
      action: "activate-device-key",
      requestId: newRequestId(),
    }) ?? Promise.resolve({ ok: false, code: "UNAVAILABLE" });
  }

  function configureDockMenu(): void {
    if (scope.__incodexPlatform !== "darwin" || scope.__incodexDockMenuConfigured) return;
    const request = scope.incodex?.requestIncognitoAction;
    if (!request) return;
    scope.__incodexDockMenuConfigured = true;
    void request({
      action: "configure-dock-menu",
      label: translateMenuLabel(isIncognitoWindow() ? "title" : "open"),
      requestId: newRequestId(),
    })
      .then((result) => {
        if (!result?.ok) scope.__incodexDockMenuConfigured = false;
      })
      .catch(() => {
        scope.__incodexDockMenuConfigured = false;
      });
  }

  function configureStatusMenu(): void {
    if (scope.__incodexPlatform !== "darwin" || scope.__incodexStatusMenuConfigured) return;
    const request = scope.incodex?.requestIncognitoAction;
    if (!request) return;
    scope.__incodexStatusMenuConfigured = true;
    void request({
      action: "configure-status-menu",
      label: translateMenuLabel(isIncognitoWindow() ? "title" : "open"),
      requestId: newRequestId(),
    })
      .then((result) => {
        if (!result?.ok) scope.__incodexStatusMenuConfigured = false;
      })
      .catch(() => {
        scope.__incodexStatusMenuConfigured = false;
      });
  }

  return {
    requestAction,
    requestDeviceKeyActivation,
    configureDockMenu,
    configureStatusMenu,
    isIncognitoWindow,
    shortcutLabel,
  };
}
