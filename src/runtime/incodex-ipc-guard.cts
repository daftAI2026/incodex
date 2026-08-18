// @ts-nocheck
"use strict";

const ALLOWED_HOSTS = new Set(["chatgpt.com", "openai.com"]);

function hostAllowed(hostname) {
  if (!hostname) return false;
  if (ALLOWED_HOSTS.has(hostname)) return true;
  return [...ALLOWED_HOSTS].some((root) => hostname.endsWith(`.${root}`));
}

function urlAllowed(raw) {
  if (!raw || typeof raw !== "string") return false;
  let parsed;
  try {
    parsed = new URL(raw);
  } catch {
    return false;
  }
  if (parsed.protocol === "file:" || parsed.protocol === "app:" || parsed.protocol === "codex:") {
    return true;
  }
  if (parsed.protocol === "https:" && hostAllowed(parsed.hostname)) return true;
  return false;
}

function authorizeSender(snapshot, allowlist) {
  if (!snapshot?.hasFrame) return { ok: false, code: "NO_SENDER_FRAME" };
  if (snapshot.isDestroyed) return { ok: false, code: "SENDER_DESTROYED" };
  if (!snapshot.isMainFrame) return { ok: false, code: "NOT_TOP_FRAME" };
  if (!urlAllowed(snapshot.url)) return { ok: false, code: "URL_NOT_ALLOWED" };
  if (snapshot.windowId == null || !allowlist.has(snapshot.windowId)) {
    return { ok: false, code: "WINDOW_NOT_ALLOWED" };
  }
  return { ok: true };
}

function snapshotFromEvent(event) {
  const frame = event?.senderFrame;
  const contents = event?.sender;
  let windowId = null;
  try {
    const { BrowserWindow } = require("electron");
    windowId = BrowserWindow.fromWebContents(contents)?.id ?? null;
  } catch {
    windowId = null;
  }
  const mainFrame = contents?.mainFrame;
  return {
    hasFrame: Boolean(frame),
    isDestroyed: Boolean(frame?.isDestroyed?.() || contents?.isDestroyed?.()),
    isMainFrame: Boolean(frame && mainFrame && frame === mainFrame && !frame.parent),
    url: frame?.url || contents?.getURL?.() || "",
    windowId,
    partition: contents?.session?.partition || null,
  };
}

function actionResponse(requestId, result) {
  return {
    requestId: requestId || "",
    ok: result.ok === true,
    code: result.code || (result.ok ? "OK" : "FAILED"),
    reason: result.reason,
  };
}

export { urlAllowed, authorizeSender, snapshotFromEvent, actionResponse };
