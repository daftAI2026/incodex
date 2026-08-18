// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
const { contextBridge, ipcRenderer } = require("electron");
function isTopFrame() {
    try {
        return window === window.top;
    }
    catch {
        return false;
    }
}
if (!isTopFrame()) {
    module.exports = {};
}
else {
    const api = {
        /** @param {{ action: string, requestId?: string }} payload */
        requestIncognitoAction: (payload) => ipcRenderer.invoke("incodex-action", payload),
    };
    try {
        contextBridge.exposeInMainWorld("incodex", api);
    }
    catch {
        globalThis.incodex = api;
    }
}
