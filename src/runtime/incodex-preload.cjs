"use strict";

const { contextBridge, ipcRenderer } = require("electron");

const api = {
  isIncognito: () => ipcRenderer.sendSync("incodex-is-incognito") === true,
  openIncognito: () => ipcRenderer.invoke("incodex-open-incognito"),
  quitIncognito: () => ipcRenderer.invoke("incodex-quit-incognito"),
};

try {
  contextBridge.exposeInMainWorld("incodex", api);
} catch {
  globalThis.incodex = api;
}
