/**
 * [INPUT]: 依赖注入的 Module、sky.node 导出与 Electron 事件源。
 * [OUTPUT]: 验证退出诊断只观察官方 getter，不改加载、返回或退出行为。
 * [POS]: incodex-quit-trace 的纯夹具回归；不加载原生模块、不操作 App。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { EventEmitter } from "node:events";
import { createQuitTrace } from "./incodex-quit-trace.cts";

const SKY_PATH = "/Applications/ChatGPT.app/Contents/Resources/native/sky.node";
const GETTER = "isPrivacySettingsTerminationRequest";

function harness(exportsValue: Record<string, unknown> = {}, cache = false, loadFailure?: Error) {
  const calls: Array<{ receiver: unknown; args: unknown[] }> = [];
  const records: Array<[string, boolean]> = [];
  const moduleApi: any = {
    _cache: Object.create(null),
    _load(this: unknown, ...args: unknown[]) {
      calls.push({ receiver: this, args });
      if (loadFailure && args[0] === SKY_PATH) throw loadFailure;
      return exportsValue;
    },
  };
  if (cache) moduleApi._cache[SKY_PATH] = { exports: exportsValue };
  const originalLoad = moduleApi._load;
  const trace = createQuitTrace({
    moduleApi,
    skyPath: SKY_PATH,
    record: (code: string, value: boolean) => records.push([code, value]),
  });
  return { calls, records, moduleApi, originalLoad, trace };
}

test("exact sky load preserves Module._load receiver, arguments and exports identity", () => {
  const getterCalls: Array<{ receiver: unknown; args: unknown[] }> = [];
  const nativeExports = {
    [GETTER]: function (this: unknown, ...args: unknown[]) {
      getterCalls.push({ receiver: this, args });
      return true;
    },
    unrelated: 7,
  };
  const originalGetter = nativeExports[GETTER];
  const h = harness(nativeExports);
  const parent = { id: "fixture-parent" };
  const receiver = { id: "module-loader" };

  const loaded = h.moduleApi._load.call(receiver, SKY_PATH, parent, true);

  expect(loaded).toBe(nativeExports);
  expect(h.calls).toEqual([{ receiver, args: [SKY_PATH, parent, true] }]);
  expect(h.moduleApi._load).toBe(h.originalLoad);
  expect(nativeExports[GETTER].apply(receiver, ["fixture-arg"])).toBe(true);
  expect(getterCalls).toEqual([{ receiver, args: ["fixture-arg"] }]);
  expect(h.records).toEqual([["getter-value", true]]);
  h.trace.dispose();
  expect(nativeExports[GETTER]).toBe(originalGetter);
});

test("unrelated loads stay observed and a matching thrown load retires without changing the error", () => {
  const failure = new Error("fixture-only failure");
  const h = harness({}, false, failure);
  const parent = { id: "fixture-parent" };
  expect(h.moduleApi._load("/other/native.node", parent, false)).toEqual({});
  expect(h.moduleApi._load).not.toBe(h.originalLoad);

  expect(() => h.moduleApi._load.call(h.moduleApi, SKY_PATH, parent, false)).toThrow(failure);
  expect(h.moduleApi._load).toBe(h.originalLoad);
  expect(h.records).toEqual([]);
});

test("an already cached sky export is observed in place without replacing the loader", () => {
  const nativeExports = { [GETTER]: () => false, sibling: { unchanged: true } };
  const originalGetter = nativeExports[GETTER];
  const h = harness(nativeExports, true);
  const loader = h.moduleApi._load;
  const sibling = nativeExports.sibling;
  expect(h.moduleApi._load).toBe(loader);
  expect(nativeExports).toBe(h.moduleApi._cache[SKY_PATH].exports);
  expect(nativeExports.sibling).toBe(sibling);
  expect(nativeExports[GETTER]()).toBe(false);
  expect(h.records).toEqual([["getter-value", false]]);
  h.trace.dispose();
  expect(nativeExports[GETTER]).toBe(originalGetter);
});

test("unreadable getter descriptors are reported without invoking accessors or bypassing them", () => {
  let accessorReads = 0;
  const nativeExports = {};
  Object.defineProperty(nativeExports, GETTER, {
    configurable: false,
    enumerable: true,
    get() { accessorReads++; throw new Error("must not read accessor"); },
  });
  const h = harness(nativeExports, true);

  expect(accessorReads).toBe(0);
  expect(h.records).toEqual([["getter-unavailable", false]]);
  expect(accessorReads).toBe(0);
});

test("non-writable native getter is unavailable and stays unchanged", () => {
  const getter = () => true;
  const nativeExports = {};
  Object.defineProperty(nativeExports, GETTER, { configurable: false, enumerable: true, value: getter, writable: false });
  const h = harness(nativeExports, true);

  expect(Object.getOwnPropertyDescriptor(nativeExports, GETTER)).toEqual({
    configurable: false,
    enumerable: true,
    value: getter,
    writable: false,
  });
  expect(h.records).toEqual([["getter-unavailable", false]]);
});

test("getter exceptions log only a closed code and preserve the original exception", () => {
  const failure = new Error("fixture-only getter failure");
  const nativeExports = { [GETTER]: () => { throw failure; } };
  const h = harness(nativeExports, true);
  expect(() => nativeExports[GETTER]()).toThrow(failure);
  expect(h.records).toEqual([["getter-threw", false]]);
  h.trace.dispose();
});

test("unexpected getter values are returned unchanged and logged as a bounded failure", () => {
  let conversions = 0;
  const value = {};
  Object.defineProperties(value, {
    valueOf: { value() { conversions++; throw new Error("must not coerce result"); } },
    toString: { value() { conversions++; throw new Error("must not serialize result"); } },
    [Symbol.toPrimitive]: { value() { conversions++; throw new Error("must not stringify result"); } },
  });
  const nativeExports = { [GETTER]: () => value };
  const h = harness(nativeExports, true);

  expect(nativeExports[GETTER]("private argument")).toBe(value);
  expect(conversions).toBe(0);
  expect(h.records).toEqual([["getter-nonboolean", false]]);
  h.trace.dispose();
});

test("logger exceptions preserve the official getter return and exception", () => {
  const success = { [GETTER]: () => 17 };
  const g = harness(success, true);
  g.trace.dispose();
  const successTrace = createQuitTrace({ moduleApi: g.moduleApi, skyPath: SKY_PATH, record: () => { throw new Error("logger failure"); } });
  expect(success[GETTER]()).toBe(17);

  const failure = new Error("fixture-only getter failure");
  const throws = { [GETTER]: () => { throw failure; } };
  const h = harness(throws, true);
  h.trace.dispose();
  createQuitTrace({ moduleApi: h.moduleApi, skyPath: SKY_PATH, record: () => { throw new Error("logger failure"); } });
  expect(() => throws[GETTER]()).toThrow(failure);
  successTrace.dispose();
});

test("lifecycle listener observes an earlier official veto and never changes the event", () => {
  const h = harness({});
  const app = new EventEmitter();
  let preventCalls = 0;
  const event = {
    defaultPrevented: false,
    preventDefault() { preventCalls++; this.defaultPrevented = true; },
  };
  app.on("before-quit", current => current.preventDefault());
  h.trace.attachLifecycle(app);

  app.emit("before-quit", event);
  app.emit("will-quit", { defaultPrevented: false, preventDefault() { preventCalls++; } });
  app.emit("activate", { defaultPrevented: true, preventDefault() { preventCalls++; } });

  expect(h.records).toEqual([
    ["app-before-quit", true],
    ["app-will-quit", false],
    ["app-activate", true],
  ]);
  expect(preventCalls).toBe(1);
  expect(event.defaultPrevented).toBe(true);
  h.trace.dispose();
  expect(app.listenerCount("before-quit")).toBe(1);
});
