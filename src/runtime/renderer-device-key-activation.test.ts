/**
 * [INPUT]: 依赖合成 actions ACK、renderer generation scope 与计时器。
 * [OUTPUT]: 验证 token 化 UNKNOWN_ACTION 重试及旧 B 成功竞态。
 * [POS]: Injector与已提交main actions的交接回归；不访问App或native adapter。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { createRendererActionBridge } from "./renderer-action-bridge.ts";

const helperModule = () => import("./renderer-device-key-activation.ts").catch(() => null);
const ACTION_C = `activate-device-key:${"c".repeat(64)}`;

function scopeFixture() {
  let nextId = 0;
  const timers = new Map<number, () => void>();
  const listeners = new Map<string, Set<() => void>>();
  const scope = {
    addEventListener(type: string, handler: () => void) {
      const set = listeners.get(type) ?? new Set(); set.add(handler); listeners.set(type, set);
    },
    removeEventListener(type: string, handler: () => void) { listeners.get(type)?.delete(handler); },
    setTimeout(handler: () => void) { const id = ++nextId; timers.set(id, handler); return id; },
    clearTimeout(id: number) { timers.delete(id); },
  };
  return { scope: scope as unknown as Pick<Window, "addEventListener" | "removeEventListener" | "setTimeout" | "clearTimeout">,
    timers, listeners, fire(id: number) { const handler = timers.get(id); timers.delete(id); handler?.(); } };
}

async function settleMicrotasks() {
  for (let index = 0; index < 8; index++) await Promise.resolve();
}

test("unknown action retries until the committed action responds, then stops", async () => {
  const mod = await helperModule();
  expect(typeof mod?.createDeviceKeyActivationRetry).toBe("function");
  if (!mod) return;
  const f = scopeFixture();
  let committed = false;
  const responses: Array<{ action: string; requestId: string }> = [];
  const activation = mod.createDeviceKeyActivationRetry({
    platform: "darwin", incognito: false, scope: f.scope,
    request: () => {
      responses.push({ action: ACTION_C, requestId: `id-${responses.length}` });
      return Promise.resolve(committed ? { ok: true, code: "INSTALLED" } : { ok: false, code: "UNKNOWN_ACTION" });
    },
  });
  activation.start();
  await settleMicrotasks();
  expect(responses).toHaveLength(1);
  expect([...f.timers.values()]).toHaveLength(1);
  committed = true;
  f.fire([...f.timers.keys()][0]!);
  await settleMicrotasks();
  expect(responses).toHaveLength(2);
  expect(responses.every(value => Object.keys(value).sort().join() === "action,requestId")).toBe(true);
  expect(f.timers.size).toBe(0);
  activation.dispose();
});

test("an old B success cannot suppress C activation after the action token changes", async () => {
  const mod = await helperModule();
  expect(typeof mod?.createDeviceKeyActivationRetry).toBe("function");
  if (!mod) return;
  const f = scopeFixture();
  const actionC = ACTION_C;
  const legacyActionB = "activate-device-key";
  let committed = false;
  let cActivations = 0;
  const sent: Array<{ action: string; requestId: string }> = [];
  const windowScope = Object.assign(f.scope, {
    incodex: {
      requestIncognitoAction: async (payload: { action: string; requestId: string }) => {
        sent.push(payload);
        if (!committed) {
          return payload.action === legacyActionB
            ? { ok: true, code: "ALREADY_ACTIVE" }
            : { ok: false, code: "UNKNOWN_ACTION" };
        }
        if (payload.action === actionC) {
          cActivations++;
          return { ok: true, code: "INSTALLED" };
        }
        return { ok: false, code: "UNKNOWN_ACTION" };
      },
    },
  }) as unknown as Window;
  const bridge = createRendererActionBridge(windowScope, key => key, actionC as `activate-device-key:${string}`);
  const activation = mod.createDeviceKeyActivationRetry({
    platform: "darwin", incognito: false, scope: windowScope, request: bridge.requestDeviceKeyActivation,
  });

  activation.start();
  await settleMicrotasks();
  expect(sent.map(({ action }) => action)).toEqual([actionC]);
  expect(sent[0]).toMatchObject({ action: actionC });
  expect(Object.keys(sent[0]!).sort()).toEqual(["action", "requestId"]);
  expect(f.timers.size).toBe(1);
  committed = true;
  f.fire([...f.timers.keys()][0]!);
  await settleMicrotasks();
  expect(sent.map(({ action }) => action)).toEqual([actionC, actionC]);
  expect(cActivations).toBe(1);
  expect(f.timers.size).toBe(0);
  activation.dispose();
});

test("only UNKNOWN_ACTION retries, and pagehide cancels a queued attempt", async () => {
  const mod = await helperModule();
  expect(typeof mod?.createDeviceKeyActivationRetry).toBe("function");
  if (!mod) return;
  const failed = scopeFixture();
  let calls = 0;
  const activation = mod.createDeviceKeyActivationRetry({
    platform: "darwin", incognito: false, scope: failed.scope,
    request: () => { calls++; return Promise.resolve({ ok: false, code: "UNAVAILABLE" }); },
  });
  activation.start(); await settleMicrotasks();
  expect(calls).toBe(1);
  expect(failed.timers.size).toBe(0);

  const queued = scopeFixture();
  const canceled = mod.createDeviceKeyActivationRetry({
    platform: "darwin", incognito: false, scope: queued.scope,
    request: async () => ({ ok: false, code: "UNKNOWN_ACTION" }),
  });
  canceled.start(); await settleMicrotasks();
  expect(queued.timers.size).toBe(1);
  for (const handler of queued.listeners.get("pagehide") ?? []) handler();
  expect(queued.timers.size).toBe(0);
  expect(queued.listeners.get("pagehide")?.size).toBe(0);
});
