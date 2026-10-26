/**
 * [INPUT]: 依赖合成 renderer 代际与可控激活回调。
 * [OUTPUT]: 验证ACK、切换、回滚、token动作重试退休与旧回调取消。
 * [POS]: renderer代际隔离回归，不操作真实窗口或native adapter。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { beforeAll, describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import vm from "node:vm";

const entry = `${import.meta.dir}/inject.ts`;
const original = readFileSync(entry, "utf8");
const sources = new Map<string, string>();
const DEVICE_KEY_ACTIVATION_ACTION = "activate-device-key:__INCODEX_DEVICE_KEY_ACTIVATION_TOKEN__";

beforeAll(async () => {
  for (const id of ["A", "B", "broken"]) {
    const instrumented = original.replace(
      "function onKeydown(event: KeyboardEvent): void {",
      `function onKeydown(event: KeyboardEvent): void { window.__testHandledBy = ${JSON.stringify(id)};`,
    ).replace("function ensureStyle(): void {", id === "broken"
      ? 'function ensureStyle(): void { throw new Error("candidate activation failed");'
      : "function ensureStyle(): void {");
    const result = await Bun.build({
      entrypoints: [entry], target: "browser", format: "iife",
      plugins: [{ name: "generation-fixture", setup(builder) {
        builder.onLoad({ filter: /[/\\]inject\.ts$/ }, () => ({ contents: instrumented, loader: "ts" }));
      } }],
    });
    if (!result.success) throw new AggregateError(result.logs);
    const bundle = await result.outputs[0]!.text();
    expect(bundle).toContain("__testHandledBy");
    if (id === "broken") expect(bundle).toContain("candidate activation failed");
    sources.set(id, bundle);
  }
});

function rendererFixture(loading = false) {
  const listeners = new Map<string, Set<(event: any) => void>>();
  const domListeners = new Map<string, Set<() => void>>();
  const observers: Array<{ live: boolean; callback: () => void; options?: MutationObserverInit }> = [];
  const frames: Array<() => void> = [];
  const timers = new Map<number, () => void>();
  let timerId = 0;
  const styles = new Map<string, any>();
  let queryCount = 0;
  let actions = 0;
  const document: any = {
    readyState: loading ? "loading" : "complete", styleSheets: [],
    documentElement: { lang: "en", setAttribute() {}, getAttribute: () => null },
    head: { append(element: any) { styles.set(element.id, element); } },
    createElement: () => ({ textContent: "", remove() { styles.delete(this.id); }, id: "" }),
    getElementById: (id: string) => styles.get(id) ?? null,
    querySelector() { queryCount++; return null; },
    querySelectorAll() { queryCount++; return []; },
    addEventListener(type: string, handler: () => void) {
      const set = domListeners.get(type) ?? new Set(); set.add(handler); domListeners.set(type, set);
    },
    removeEventListener(type: string, handler: () => void) { domListeners.get(type)?.delete(handler); },
  };
  const window: any = {
    __incodexIncognito: false,
    addEventListener(type: string, handler: (event: any) => void) {
      const set = listeners.get(type) ?? new Set(); set.add(handler); listeners.set(type, set);
    },
    removeEventListener(type: string, handler: (event: any) => void) { listeners.get(type)?.delete(handler); },
    setTimeout(handler: () => void) { const id = ++timerId; timers.set(id, handler); return id; },
    clearTimeout(id: number) { timers.delete(id); },
    incodex: { requestIncognitoAction: async () => { actions++; return { ok: true }; } },
  };
  const context = vm.createContext({
    window, document, navigator: { language: "en" },
    console: { warn() {} }, URL, TextEncoder, TextDecoder, setTimeout, clearTimeout,
    requestAnimationFrame: (callback: () => void) => { frames.push(callback); return frames.length; },
    cancelAnimationFrame() {},
    MutationObserver: class {
      item: { live: boolean; callback: () => void; options?: MutationObserverInit };
      constructor(callback: () => void) { this.item = { live: false, callback }; observers.push(this.item); }
      observe(_target: unknown, options: MutationObserverInit) { this.item.live = true; this.item.options = options; }
      disconnect() { this.item.live = false; }
    },
  });
  function evaluate(id: string) {
    window.__incodexRendererRequest = { protocol: 1, id };
    return vm.runInContext(sources.get(id)!, context);
  }
  async function keydown() {
    const event = { key: "n", code: "KeyN", ctrlKey: true, shiftKey: true,
      preventDefault() {}, stopImmediatePropagation() {} };
    for (const handler of listeners.get("keydown") ?? []) handler(event);
    await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
  }
  return { window, document, evaluate, keydown, listeners, domListeners, observers, frames,
    queryCount: () => queryCount, actions: () => actions, timers };
}

describe("actual shared injector generation lifecycle", () => {
  test("same-generation reinjection preserves the sidebar manager and activation handler", () => {
    const f = rendererFixture(); f.evaluate("A");
    const manager = f.window.__incodexSidebarEntry, activate = f.window.__incodexSidebarActivate;
    f.evaluate("A");
    expect(f.window.__incodexSidebarEntry).toBe(manager);
    expect(f.window.__incodexSidebarActivate).toBe(activate);
    expect(f.listeners.get("keydown")?.size).toBe(1);
  });

  test("Mac action retry crosses a generation commit and retirement cancels its timer", async () => {
    const f = rendererFixture();
    f.window.__incodexPlatform = "darwin";
    const payloads: any[] = [];
    f.window.incodex.requestIncognitoAction = async (payload: any) => {
      if (payload.action !== DEVICE_KEY_ACTIVATION_ACTION) return { ok: true, code: "OK" };
      payloads.push(payload);
      return payloads.length === 1
        ? { ok: false, code: "UNKNOWN_ACTION" }
        : { ok: true, code: "INSTALLED" };
    };
    f.evaluate("A");
    for (let index = 0; index < 8; index++) await Promise.resolve();
    expect(payloads).toHaveLength(1);
    expect(Object.keys(payloads[0]).sort()).toEqual(["action", "requestId"]);
    expect(payloads[0].action).toBe(DEVICE_KEY_ACTIVATION_ACTION);
    expect(f.timers.size).toBe(1);

    f.evaluate("B");
    for (let index = 0; index < 8; index++) await Promise.resolve();
    expect(payloads).toHaveLength(2);
    expect(f.timers.size).toBe(0);
  });

  test("a second generation replaces callable hooks without duplicate window listeners", async () => {
    const f = rendererFixture();
    f.evaluate("A"); await f.keydown();
    expect(f.window.__testHandledBy).toBe("A");
    f.evaluate("B"); await f.keydown();
    expect(f.window.__testHandledBy).toBe("B");
    expect(f.actions()).toBe(2);
    for (const type of ["keydown", "blur", "focus", "codex:dismiss-tooltips"]) {
      expect(f.listeners.get(type)?.size, type).toBe(1);
    }
    expect(f.window.__incodexRendererGeneration).toMatchObject({ protocol: 1, id: "B" });
  });

  test("retired observers and queued animation frames cannot reconcile old UI", () => {
    const f = rendererFixture(); f.evaluate("A");
    const old = [...f.observers];
    old.at(-1)!.callback();
    const pending = [...f.frames];
    f.evaluate("B");
    expect(old.every(observer => !observer.live)).toBe(true);
    const before = f.queryCount();
    for (const callback of pending) callback();
    expect(f.queryCount()).toBe(before);
  });

  test("failed activation restores the previous hooks and observer ownership", async () => {
    const f = rendererFixture(); f.evaluate("A");
    expect(() => f.evaluate("broken")).toThrow("candidate activation failed");
    await f.keydown();
    expect(f.window.__testHandledBy).toBe("A");
    expect(f.window.__incodexRendererGeneration).toMatchObject({ id: "A" });
    expect(f.listeners.get("keydown")?.size).toBe(1);
    expect(f.observers.filter(observer => observer.live)).toHaveLength(2);
  });

  test("before DOM readiness only the latest selected generation starts", async () => {
    const f = rendererFixture(true); f.evaluate("A"); f.evaluate("B");
    expect(f.window.__incodexRendererGeneration).toBeUndefined();
    f.document.readyState = "complete";
    for (const callback of f.domListeners.get("DOMContentLoaded") ?? []) callback();
    await f.keydown();
    expect(f.window.__testHandledBy).toBe("B");
    expect(f.listeners.get("keydown")?.size).toBe(1);
  });

  test("an older pending request settles through the live UI hooks exactly once", async () => {
    const f = rendererFixture(); let settle!: (value: { ok: boolean }) => void;
    f.window.incodex.requestIncognitoAction = () => new Promise(resolve => { settle = resolve; });
    f.evaluate("A"); await f.keydown(); f.evaluate("B");
    const before = f.queryCount();
    settle({ ok: false });
    for (let i = 0; i < 10; i++) await Promise.resolve();
    // The current notification manager tries to locate the official toaster.
    // Retired A must not mount UI; B still presents the real request outcome.
    expect(f.queryCount()).toBeGreaterThan(before);
    expect(f.window.__incodexNotifications.errorPending()).toBe(true);
    expect(f.window.__incodexRendererGeneration.id).toBe("B");
  });

  for (const nextOk of [false, true]) {
    test(`a new request clears old failure feedback before its ${nextOk ? "success" : "failure"} settles`, async () => {
      const f = rendererFixture();
      const pending: Array<(value: { ok: boolean }) => void> = [];
      f.window.incodex.requestIncognitoAction = () => new Promise(resolve => { pending.push(resolve); });
      f.evaluate("A");

      await f.keydown();
      expect(pending).toHaveLength(1);
      pending[0]!({ ok: false });
      for (let i = 0; i < 10; i++) await Promise.resolve();
      expect(f.window.__incodexNotifications.errorPending()).toBe(true);

      await f.keydown();
      expect(pending).toHaveLength(2);
      expect(f.window.__incodexNotifications.errorPending()).toBe(false);

      pending[1]!({ ok: nextOk });
      for (let i = 0; i < 10; i++) await Promise.resolve();
      expect(f.window.__incodexNotifications.errorPending()).toBe(!nextOk);
    });
  }

  for (const newerOk of [true, false]) {
    test(`an older completion cannot replace newer ${newerOk ? "success" : "failure"} feedback`, async () => {
      const f = rendererFixture();
      const pending: Array<(value: { ok: boolean }) => void> = [];
      f.window.incodex.requestIncognitoAction = () => new Promise(resolve => { pending.push(resolve); });
      f.evaluate("A"); await f.keydown();
      f.evaluate("B"); await f.keydown();
      expect(pending).toHaveLength(2);
      pending[1]!({ ok: newerOk });
      for (let i = 0; i < 10; i++) await Promise.resolve();
      expect(f.window.__incodexNotifications.errorPending()).toBe(!newerOk);
      pending[0]!({ ok: !newerOk });
      for (let i = 0; i < 10; i++) await Promise.resolve();
      expect(f.window.__incodexNotifications.errorPending()).toBe(!newerOk);
    });
  }

  test("failed activation restores pending feedback, and switching respects dismissal", async () => {
    const f = rendererFixture();
    f.window.incodex.requestIncognitoAction = async () => ({ ok: false });
    f.evaluate("A"); await f.keydown();
    expect(f.window.__incodexNotifications.errorPending()).toBe(true);
    expect(() => f.evaluate("broken")).toThrow("candidate activation failed");
    expect(f.window.__incodexNotifications.errorPending()).toBe(true);
    f.evaluate("B");
    expect(f.window.__incodexNotifications.errorPending()).toBe(true);
    f.window.__incodexNotifications.hideError();
    f.evaluate("A");
    expect(f.window.__incodexNotifications.errorPending()).toBe(false);
  });

  test("an already running incognito renderer retains its generation", async () => {
    const f = rendererFixture(); f.evaluate("A");
    f.window.__incodexIncognito = true;
    f.evaluate("B"); await f.keydown();
    expect(f.window.__testHandledBy).toBe("A");
    expect(f.window.__incodexRendererGeneration).toMatchObject({ id: "A", restartRequired: true });
  });

  test("a legacy running injector requires restart instead of adding a second event owner", () => {
    const f = rendererFixture(); f.window.__incodexStarted = true;
    f.evaluate("B");
    expect(f.listeners.size).toBe(0);
    expect(f.window.__incodexRendererGeneration?.restartRequired).toBe(true);
  });

  test("same-generation refresh reobserves when native CDP enables masking later", () => {
    const f = rendererFixture(); f.evaluate("A");
    f.window.__incodexIncognito = true;
    f.window.__incodexProfileMask = {};
    f.evaluate("A");
    expect(f.observers.filter(observer => observer.live && observer.options?.attributes)).toHaveLength(1);
    expect(f.listeners.get("keydown")?.size).toBe(1);
  });

  test("stable blur/focus/dismiss entry points always address the live tooltip", () => {
    const f = rendererFixture(); f.evaluate("A"); f.evaluate("B");
    const calls: string[] = [];
    f.window.__incodexTooltipState.lifecycle = {
      windowBlur: () => calls.push("blur"), windowFocus: () => calls.push("focus"),
      dismiss: () => calls.push("dismiss"),
    };
    for (const type of ["blur", "focus", "codex:dismiss-tooltips"]) {
      for (const handler of f.listeners.get(type) ?? []) handler({});
    }
    expect(calls).toEqual(["blur", "focus", "dismiss"]);
  });
});
