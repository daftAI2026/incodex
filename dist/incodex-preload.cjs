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
    requestIncognitoAction: (payload) => ipcRenderer.invoke("incodex-action", payload),
  };
  try {
    contextBridge.exposeInMainWorld("incodex", api);
  } catch {
    globalThis.incodex = api;
  }
}
