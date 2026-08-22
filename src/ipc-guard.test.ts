import { describe, expect, test } from "bun:test";
import {
  actionResponse,
  authorizeSender,
  bindWindowIdentity,
  urlAllowed,
} from "./runtime/incodex-ipc-guard.cts";

const mainSession = {};
const packagedOrigin = "file:///Applications/ChatGPT.app/Contents/Resources/app.asar";
const trustedOrigins = new Set(["app://-", packagedOrigin, "https://chatgpt.com"]);

function identity(over: Record<string, unknown> = {}) {
  return {
    webContentsId: 11,
    session: mainSession,
    frameProcessId: 22,
    frameRoutingId: 33,
    origin: packagedOrigin,
    ...over,
  };
}

const allow = new Map([[1, identity()]]);

function snap(over: Record<string, unknown> = {}) {
  return {
    hasFrame: true,
    isDestroyed: false,
    isMainFrame: true,
    url: "file:///Applications/ChatGPT.app/Contents/Resources/app.asar/index.html",
    windowId: 1,
    webContentsId: 11,
    session: mainSession,
    frameProcessId: 22,
    frameRoutingId: 33,
    ...over,
  };
}

describe("authorizeSender", () => {
  test("allows an allowlisted top frame on a local file URL", () => {
    expect(authorizeSender(snap(), allow)).toEqual({ ok: true });
  });

  test("iframe cannot call open or quit", () => {
    expect(authorizeSender(snap({ isMainFrame: false }), allow)).toEqual({
      ok: false,
      code: "NOT_TOP_FRAME",
    });
  });

  test("missing sender frame is rejected", () => {
    expect(authorizeSender(snap({ hasFrame: false }), allow).code).toBe("NO_SENDER_FRAME");
  });

  test("data URL frame cannot call", () => {
    expect(authorizeSender(snap({ url: "data:text/html,pwn" }), allow).code).toBe("URL_NOT_ALLOWED");
  });

  test("javascript and blob URLs cannot call", () => {
    expect(authorizeSender(snap({ url: "javascript:alert(1)" }), allow).code).toBe("URL_NOT_ALLOWED");
    expect(authorizeSender(snap({ url: "blob:https://evil.test/1" }), allow).code).toBe("URL_NOT_ALLOWED");
  });

  test("a frame that navigated away cannot call", () => {
    expect(authorizeSender(snap({ url: "https://evil.example/hook" }), allow).code).toBe("URL_NOT_ALLOWED");
  });

  test("popup or other window outside the allowlist cannot call", () => {
    expect(authorizeSender(snap({ windowId: 99 }), allow).code).toBe("WINDOW_NOT_ALLOWED");
  });

  test("a reused window id cannot substitute different web contents", () => {
    expect(authorizeSender(snap({ webContentsId: 12 }), allow).code).toBe(
      "WEB_CONTENTS_NOT_ALLOWED",
    );
  });

  test("a different Electron session cannot reuse an allowed window", () => {
    expect(authorizeSender(snap({ session: {} }), allow).code).toBe("SESSION_NOT_ALLOWED");
  });

  test("a replaced main frame cannot reuse the previous frame identity", () => {
    expect(authorizeSender(snap({ frameRoutingId: 34 }), allow).code).toBe(
      "FRAME_NOT_ALLOWED",
    );
  });

  test("navigation to another origin revokes the action bridge", () => {
    expect(authorizeSender(snap({ url: "https://chatgpt.com/codex" }), allow).code).toBe(
      "ORIGIN_NOT_ALLOWED",
    );
  });

  test("same-origin routes and fragments keep the recorded sender identity", () => {
    const httpsAllow = new Map([[1, identity({ origin: "https://chatgpt.com" })]]);
    expect(
      authorizeSender(snap({ url: "https://chatgpt.com/codex/thread/1#reply" }), httpsAllow),
    ).toEqual({ ok: true });
  });
});

describe("urlAllowed", () => {
  test("allows only the packaged app roots and exact hosted app origin", () => {
    expect(urlAllowed("app://-/index.html", trustedOrigins)).toBe(true);
    expect(
      urlAllowed(
        "file:///Applications/ChatGPT.app/Contents/Resources/app.asar/index.html",
        trustedOrigins,
      ),
    ).toBe(true);
    expect(urlAllowed("https://chatgpt.com/codex", trustedOrigins)).toBe(true);
    expect(urlAllowed("app://evil.local/index.html", trustedOrigins)).toBe(false);
    expect(urlAllowed("file:///tmp/attacker.html", trustedOrigins)).toBe(false);
    expect(
      urlAllowed("file:///tmp/Evil.app/Contents/Resources/app.asar/index.html", trustedOrigins),
    ).toBe(false);
    expect(urlAllowed("https://auth.openai.com/login", trustedOrigins)).toBe(false);
    expect(urlAllowed("https://evil.chatgpt.com/codex", trustedOrigins)).toBe(false);
  });

  test("beacon-style and opaque URLs are not enough", () => {
    expect(urlAllowed("https://incodex.invalid/open", trustedOrigins)).toBe(false);
    expect(urlAllowed("", trustedOrigins)).toBe(false);
  });
});

describe("bindWindowIdentity", () => {
  function windowFixture() {
    const session = {};
    const state = {
      url: "app://-/index.html",
      frameProcessId: 22,
      frameRoutingId: 33,
    };
    return {
      state,
      win: {
        id: 1,
        webContents: {
          id: 11,
          session,
          isDestroyed: () => false,
          getURL: () => state.url,
          get mainFrame() {
            return {
              processId: state.frameProcessId,
              routingId: state.frameRoutingId,
            };
          },
        },
      },
    };
  }

  test("refreshes a same-origin main frame without widening its origin", () => {
    const allowed = new Map();
    const { state, win } = windowFixture();
    expect(bindWindowIdentity(allowed, win, trustedOrigins)).toBe(true);
    state.frameRoutingId = 34;
    expect(bindWindowIdentity(allowed, win, trustedOrigins)).toBe(true);
    expect(allowed.get(1)?.frameRoutingId).toBe(34);
  });

  test("cross-origin navigation permanently revokes that window identity", () => {
    const allowed = new Map();
    const { state, win } = windowFixture();
    expect(bindWindowIdentity(allowed, win, trustedOrigins)).toBe(true);
    state.url = "https://chatgpt.com/codex";
    expect(bindWindowIdentity(allowed, win, trustedOrigins)).toBe(false);
    state.url = "app://-/index.html";
    expect(bindWindowIdentity(allowed, win, trustedOrigins)).toBe(false);
  });

  test("does not bind a packaged file origin outside the current app", () => {
    const allowed = new Map();
    const { state, win } = windowFixture();
    state.url = "file:///tmp/Evil.app/Contents/Resources/app.asar/index.html";
    expect(bindWindowIdentity(allowed, win, trustedOrigins)).toBe(false);
    expect(allowed.size).toBe(0);
  });
});

describe("actionResponse", () => {
  test("always returns a request id and explicit code", () => {
    expect(actionResponse("req-1", { ok: false, code: "NOT_TOP_FRAME" })).toEqual({
      requestId: "req-1",
      ok: false,
      code: "NOT_TOP_FRAME",
      reason: undefined,
    });
  });
});
