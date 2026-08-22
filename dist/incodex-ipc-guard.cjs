// @ts-nocheck
"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.navigationOrigin = navigationOrigin;
exports.urlAllowed = urlAllowed;
exports.authorizeSender = authorizeSender;
exports.bindWindowIdentity = bindWindowIdentity;
exports.snapshotFromEvent = snapshotFromEvent;
exports.actionResponse = actionResponse;
const APP_ASAR_ROOT = /\/[^/]+\.app\/Contents\/Resources\/app\.asar(?:\/|$)/;
const guardedContents = new WeakSet();
function navigationOrigin(raw) {
    if (!raw || typeof raw !== "string")
        return null;
    let parsed;
    try {
        parsed = new URL(raw);
    }
    catch {
        return null;
    }
    if (parsed.protocol === "https:") {
        return parsed.hostname === "chatgpt.com" && !parsed.username && !parsed.password && !parsed.port
            ? parsed.origin
            : null;
    }
    if (parsed.protocol === "app:") {
        return parsed.hostname === "-" && parsed.pathname === "/index.html"
            ? `${parsed.protocol}//-`
            : null;
    }
    if (parsed.protocol !== "file:" || parsed.host)
        return null;
    const match = parsed.pathname.match(APP_ASAR_ROOT);
    if (!match || match.index == null)
        return null;
    const end = match.index + match[0].replace(/\/$/, "").length;
    return `${parsed.protocol}//${parsed.pathname.slice(0, end)}`;
}
function urlAllowed(raw, trustedOrigins) {
    const origin = navigationOrigin(raw);
    return origin !== null && Boolean(trustedOrigins?.has?.(origin));
}
function authorizeSender(snapshot, allowlist) {
    if (!snapshot?.hasFrame)
        return { ok: false, code: "NO_SENDER_FRAME" };
    if (snapshot.isDestroyed)
        return { ok: false, code: "SENDER_DESTROYED" };
    if (!snapshot.isMainFrame)
        return { ok: false, code: "NOT_TOP_FRAME" };
    const origin = navigationOrigin(snapshot.url);
    if (!origin)
        return { ok: false, code: "URL_NOT_ALLOWED" };
    const expected = snapshot.windowId == null ? null : allowlist.get(snapshot.windowId);
    if (!expected || expected.revoked) {
        return { ok: false, code: "WINDOW_NOT_ALLOWED" };
    }
    if (snapshot.webContentsId !== expected.webContentsId) {
        return { ok: false, code: "WEB_CONTENTS_NOT_ALLOWED" };
    }
    if (snapshot.session !== expected.session) {
        return { ok: false, code: "SESSION_NOT_ALLOWED" };
    }
    if (snapshot.frameProcessId !== expected.frameProcessId ||
        snapshot.frameRoutingId !== expected.frameRoutingId) {
        return { ok: false, code: "FRAME_NOT_ALLOWED" };
    }
    if (origin !== expected.origin) {
        return { ok: false, code: "ORIGIN_NOT_ALLOWED" };
    }
    return { ok: true };
}
function identityFromWindow(win) {
    const contents = win?.webContents;
    const frame = contents?.mainFrame;
    const origin = navigationOrigin(contents?.getURL?.() || "");
    if (!contents || !frame || !origin || contents.isDestroyed?.())
        return null;
    return {
        webContentsId: contents.id,
        session: contents.session,
        frameProcessId: frame.processId,
        frameRoutingId: frame.routingId,
        origin,
    };
}
function revokeWindowIdentityOnNavigation(allowlist, win, raw) {
    const current = allowlist.get(win?.id);
    if (!current || current.revoked || navigationOrigin(raw) === current.origin)
        return false;
    allowlist.set(win.id, { ...current, revoked: true });
    return true;
}
function watchWindowNavigation(allowlist, win) {
    const contents = win?.webContents;
    if (!contents?.on || guardedContents.has(contents))
        return;
    guardedContents.add(contents);
    const revoke = (details, url, _isSameDocument, isMainFrame) => {
        const modern = typeof details?.url === "string";
        if ((modern ? details.isMainFrame : isMainFrame) !== true)
            return;
        revokeWindowIdentityOnNavigation(allowlist, win, modern ? details.url : url);
    };
    contents.on("did-start-navigation", revoke);
    contents.on("will-redirect", revoke);
}
function bindWindowIdentity(allowlist, win, trustedOrigins) {
    if (!win || typeof win.id !== "number")
        return false;
    watchWindowNavigation(allowlist, win);
    const current = allowlist.get(win.id);
    if (current?.revoked)
        return false;
    const next = identityFromWindow(win);
    if (!next || !trustedOrigins?.has?.(next.origin)) {
        if (current)
            allowlist.set(win.id, { ...current, revoked: true });
        return false;
    }
    if (current &&
        (current.webContentsId !== next.webContentsId ||
            current.session !== next.session ||
            current.origin !== next.origin)) {
        allowlist.set(win.id, { ...current, revoked: true });
        return false;
    }
    allowlist.set(win.id, next);
    return true;
}
function snapshotFromEvent(event) {
    const frame = event?.senderFrame;
    const contents = event?.sender;
    let windowId = null;
    try {
        const { BrowserWindow } = require("electron");
        windowId = BrowserWindow.fromWebContents(contents)?.id ?? null;
    }
    catch {
        windowId = null;
    }
    const mainFrame = contents?.mainFrame;
    return {
        hasFrame: Boolean(frame),
        isDestroyed: Boolean(frame?.isDestroyed?.() || contents?.isDestroyed?.()),
        isMainFrame: Boolean(frame && mainFrame && frame === mainFrame && !frame.parent),
        url: frame?.url || contents?.getURL?.() || "",
        windowId,
        webContentsId: contents?.id ?? null,
        session: contents?.session ?? null,
        frameProcessId: frame?.processId ?? null,
        frameRoutingId: frame?.routingId ?? null,
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
