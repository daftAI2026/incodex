import { describe, expect, test } from "bun:test";
import { actionResponse, authorizeSender, urlAllowed } from "./runtime/incodex-ipc-guard.cts";

const allow = new Set([1]);

function snap(over: Record<string, unknown> = {}) {
  return {
    hasFrame: true,
    isDestroyed: false,
    isMainFrame: true,
    url: "file:///Applications/ChatGPT.app/Contents/Resources/app.asar/index.html",
    windowId: 1,
    partition: "persist:main",
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

  test("https chatgpt.com main window is allowed", () => {
    expect(
      authorizeSender(snap({ url: "https://chatgpt.com/codex" }), allow),
    ).toEqual({ ok: true });
  });
});

describe("urlAllowed", () => {
  test("beacon-style and opaque URLs are not enough", () => {
    expect(urlAllowed("https://incodex.invalid/open")).toBe(false);
    expect(urlAllowed("")).toBe(false);
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
