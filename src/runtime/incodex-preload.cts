// @ts-nocheck
/**
 * [INPUT]: 依赖 Electron 的 contextBridge、ipcRenderer 与顶层 frame 判定。
 * [OUTPUT]: 仅向顶层 frame 暴露 incodex.requestIncognitoAction。
 * [POS]: renderer 的窄 IPC 桥，只转发 incodex-action；来源与权限校验留在 main。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
"use strict";

const { contextBridge, ipcRenderer } = require("electron");

function isTopFrame() {
  try {
    return window === window.top;
  } catch {
    return false;
  }
}

if (!isTopFrame()) {
  module.exports = {};
} else {
  const api = {
    /** @param {{ action: string, requestId?: string }} payload */
    requestIncognitoAction(payload) {
      return ipcRenderer.invoke("incodex-action", payload);
    },
  };
  try {
    contextBridge.exposeInMainWorld("incodex", api);
  } catch {
    globalThis.incodex = api;
  }
}
