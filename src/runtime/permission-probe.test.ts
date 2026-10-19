/**
 * [INPUT]: 依赖临时私有事务目录与注入的 Electron API。
 * [OUTPUT]: 验证安装 preflight、请求绑定、只读边界与 watcher 限次重读。
 * [POS]: 权限 probe 模块回归；不访问真实 App 或 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { chmodSync, closeSync, existsSync, fsyncSync, mkdirSync, mkdtempSync, openSync, readFileSync, renameSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createPermissionProbeResponder, isInstallPermissionPreflight, readScreenPermission } from "./incodex-permission-probe.cts";

const installId = "11111111-1111-4111-8111-111111111111";
// 与生产生成器的 UUIDv4 协议一致；Rust 回归另验真实生成器到冻结字节的交接。
const requestId = "22222222-2222-4222-8222-222222222222";
const appPath = "/Applications/ChatGPT.app";
const start = "Mon Oct 12 01:00:00 2026";

test("install preflight is enabled only by a nonempty Darwin flag", () => {
  expect(isInstallPermissionPreflight({ INCODEX_INTERNAL_PERMISSION_PREFLIGHT: "request" }, "darwin")).toBe(true);
  expect(isInstallPermissionPreflight({ INCODEX_INTERNAL_PERMISSION_PREFLIGHT: "" }, "darwin")).toBe(false);
  expect(isInstallPermissionPreflight({ INCODEX_INTERNAL_PERMISSION_PREFLIGHT: "request" }, "linux")).toBe(false);
});

test("screen probe preserves raw decisions without requesting any capability", () => {
  for (const raw of ["granted", "denied", "not-determined", "restricted", "unknown"]) {
    const calls: string[] = [];
    const result = readScreenPermission({ getMediaAccessStatus: (kind: string) => { calls.push(kind); return raw; } });
    expect(result).toEqual({ rawStatus: raw, status: raw === "granted" ? "granted" : raw === "unknown" ? "unknown" : "not-granted" });
    expect(calls).toEqual(["screen"]);
  }
  expect(readScreenPermission({})).toEqual({ rawStatus: "unknown", status: "unknown" });
  expect(readScreenPermission({ getMediaAccessStatus: () => { throw new Error("unavailable"); } }).status).toBe("unknown");
  expect(readScreenPermission({ getMediaAccessStatus: () => true }).status).toBe("unknown");
});

async function fixture(run: (ctx: any) => void | Promise<void>) {
  const root = mkdtempSync(join(tmpdir(), "incodex-permission-probe-"));
  const directory = join(root, "transactions", installId);
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  chmodSync(join(root, "transactions"), 0o700);
  const marker = { schemaVersion: 1, installId, requestId, appPath, presentationOwner: "cli", state: "pending", requestedAtMs: 1000 };
  writeFileSync(join(directory, "accessibility-setup.json"), JSON.stringify(marker), { mode: 0o600 });
  let calls = 0, rawStatus = "denied";
  const responder = createPermissionProbeResponder({ root, installId, appPath, pid: 123, processStartIdentity: start,
    platform: "darwin", incognito: false, now: () => 2000, watch: false,
    versions: { electron: "42.0.0", chrome: "155.0.0.0" },
    systemPreferences: { getMediaAccessStatus: () => { calls++; return rawStatus; } } });
  const request = { schemaVersion: 1, nonce: "a".repeat(32), installId, requestId, appPath, pid: 123, processStartIdentity: start, requestedAtMs: 2000 };
  const requestPath = join(directory, "permission-probe-request.json");
  const responsePath = join(directory, "permission-probe-response.json");
  const write = (value = request) => {
    const temporary = join(directory, ".request.tmp");
    const descriptor = openSync(temporary, "wx", 0o600);
    try { writeFileSync(descriptor, JSON.stringify(value)); fsyncSync(descriptor); }
    finally { closeSync(descriptor); }
    renameSync(temporary, requestPath);
  };
  try { await run({ root, directory, marker, responder, request, requestPath, responsePath, write, calls: () => calls,
    setRawStatus: (value: string) => { rawStatus = value; } }); }
  finally { responder?.dispose(); rmSync(root, { recursive: true, force: true }); }
}

function fakeWatch() {
  const nodeFs = require("node:fs");
  const originalWatch = nodeFs.watch;
  let notify: ((event: string, filename: string | null) => void) | null = null;
  nodeFs.watch = (...args: any[]) => {
    notify = args.at(-1);
    return { on() { return this; }, close() {} };
  };
  return {
    restore() { nodeFs.watch = originalWatch; },
    notify(event: string, filename: string | null) {
      if (!notify) throw new Error("watch callback was not registered");
      notify(event, filename);
    },
  };
}

function manualTimers() {
  let nextId = 0;
  const callbacks = new Map<number, () => void>();
  return {
    setTimeout(callback: () => void) {
      const id = ++nextId;
      callbacks.set(id, callback);
      return id;
    },
    clearTimeout(id: number) { callbacks.delete(id); },
    pendingCount() { return callbacks.size; },
    runNext() {
      const next = callbacks.entries().next().value as [number, () => void] | undefined;
      if (!next) return false;
      callbacks.delete(next[0]);
      next[1]();
      return true;
    },
  };
}

test("CLI-owned request gets exact main-process observation without changing its marker", () => fixture(c => {
  const before = readFileSync(join(c.directory, "accessibility-setup.json"), "utf8");
  c.write(); c.responder.respond();
  const value = JSON.parse(readFileSync(c.responsePath, "utf8"));
  expect(value).toMatchObject({ ...c.request, rawStatus: "denied", status: "not-granted", observedAtMs: 2000,
    versions: { electron: "42.0.0", chrome: "155.0.0.0" } });
  expect(c.calls()).toBe(1);
  c.responder.respond(); expect(c.calls()).toBe(1);
  expect(readFileSync(join(c.directory, "accessibility-setup.json"), "utf8")).toBe(before);
}));

test("stale requests, wrong process generation and wrong install never invoke screen API", () => fixture(c => {
  for (const change of [{ pid: 124 }, { processStartIdentity: `${start} changed` }, { installId: requestId },
    { requestId: installId }, { requestId: "2".repeat(32) }, { appPath: "/tmp/ChatGPT.app" }, { requestedAtMs: -9999 }, { nonce: "bad" }, { command: "capture" }]) {
    c.write({ ...c.request, ...change }); c.responder.respond();
    expect(existsSync(c.responsePath)).toBe(false);
  }
  expect(c.calls()).toBe(0);
}));

test("unsafe and symlink request files cannot read or publish observations", () => fixture(c => {
  c.write(); chmodSync(c.requestPath, 0o666); c.responder.respond();
  expect(c.calls()).toBe(0);
  rmSync(c.requestPath); const outside = join(c.root, "outside");
  writeFileSync(outside, JSON.stringify(c.request), { mode: 0o600 });
  symlinkSync(outside, c.requestPath); c.responder.respond();
  expect(c.calls()).toBe(0);
  expect(existsSync(c.responsePath)).toBe(false);
}));

test("request retirement during API call discards the result", () => fixture(c => {
  c.responder.dispose();
  const retired = createPermissionProbeResponder({ root: c.root, installId, appPath, pid: 123, processStartIdentity: start,
    platform: "darwin", incognito: false, now: () => 2000, watch: false,
    systemPreferences: { getMediaAccessStatus: () => { c.write({ ...c.request, nonce: "b".repeat(32) }); return "granted"; } } });
  if (!retired) throw new Error("fixture responder was not created");
  try { c.write(); retired.respond(); expect(existsSync(c.responsePath)).toBe(false); }
  finally { retired.dispose(); }
}));

test("successive nonces and a new CLI guide replace only the dedicated safe response file", () => fixture(c => {
  c.write(); c.responder.respond();
  expect(JSON.parse(readFileSync(c.responsePath, "utf8")).status).toBe("not-granted");
  c.setRawStatus("granted");
  c.write({ ...c.request, nonce: "b".repeat(32) }); c.responder.respond();
  expect(JSON.parse(readFileSync(c.responsePath, "utf8"))).toMatchObject({ nonce: "b".repeat(32), status: "granted" });
  const nextRequestId = "33333333-3333-4333-8333-333333333333";
  writeFileSync(join(c.directory, "accessibility-setup.json"), JSON.stringify({ ...c.marker, requestId: nextRequestId }));
  c.write({ ...c.request, nonce: "c".repeat(32), requestId: nextRequestId }); c.responder.respond();
  expect(JSON.parse(readFileSync(c.responsePath, "utf8"))).toMatchObject({ nonce: "c".repeat(32), requestId: nextRequestId, status: "granted" });
  expect(c.calls()).toBe(3);
}));

test("an unsafe response cannot redirect the atomic publication", () => fixture(c => {
  const outside = join(c.root, "unrelated");
  writeFileSync(outside, "keep", { mode: 0o600 }); symlinkSync(outside, c.responsePath);
  c.write(); c.responder.respond();
  expect(readFileSync(outside, "utf8")).toBe("keep");
  expect(readFileSync(c.responsePath, "utf8")).toBe("keep");
}));

test("the event-driven responder observes new requests without polling or requesting capture", () => fixture(async c => {
  c.responder.dispose();
  const live = createPermissionProbeResponder({ root: c.root, installId, appPath, pid: 123, processStartIdentity: start,
    platform: "darwin", incognito: false, now: () => 2000,
    systemPreferences: { getMediaAccessStatus: () => "granted" } });
  if (!live) throw new Error("watch responder was not created");
  try {
    c.write();
    const deadline = Date.now() + 1500;
    while (!existsSync(c.responsePath) && Date.now() < deadline) await Bun.sleep(10);
    expect(JSON.parse(readFileSync(c.responsePath, "utf8"))).toMatchObject({ nonce: c.request.nonce, status: "granted" });
  } finally { live.dispose(); }
}));

test("the event-driven responder reads a request after a temp-only rename event", () => fixture(async c => {
  c.responder.dispose();
  const watch = fakeWatch();
  let live: ReturnType<typeof createPermissionProbeResponder> | null = null;
  try {
    live = createPermissionProbeResponder({ root: c.root, installId, appPath, pid: 123,
      processStartIdentity: start, platform: "darwin", incognito: false, now: () => 2000,
      systemPreferences: { getMediaAccessStatus: () => "granted" } });
  } finally {
    watch.restore();
  }
  if (!live) throw new Error("watch responder fixture was not created");
  try {
    c.write();
    watch.notify("rename", ".request.tmp");
    const deadline = Date.now() + 1500;
    while (!existsSync(c.responsePath) && Date.now() < deadline) await Bun.sleep(10);
    expect(existsSync(c.responsePath)).toBe(true);
    expect(JSON.parse(readFileSync(c.responsePath, "utf8"))).toMatchObject({ nonce: c.request.nonce, status: "granted" });
  } finally { live.dispose(); }
}));

test("the responder retries once when a temp-only event precedes request publication", () => fixture(c => {
  c.responder.dispose();
  const watch = fakeWatch();
  const timers = manualTimers();
  const originalSetTimeout = globalThis.setTimeout;
  const originalClearTimeout = globalThis.clearTimeout;
  let live: ReturnType<typeof createPermissionProbeResponder> | null = null;
  try {
    live = createPermissionProbeResponder({ root: c.root, installId, appPath, pid: 123,
      processStartIdentity: start, platform: "darwin", incognito: false, now: () => 2000,
      systemPreferences: { getMediaAccessStatus: () => "granted" } });
  } finally { watch.restore(); }
  if (!live) throw new Error("watch responder fixture was not created");
  (globalThis as any).setTimeout = timers.setTimeout;
  (globalThis as any).clearTimeout = timers.clearTimeout;
  try {
    watch.notify("rename", ".request.tmp");
    expect(timers.pendingCount()).toBe(1);
    timers.runNext(); // 第一读发生在目标文件发布前。
    expect(existsSync(c.responsePath)).toBe(false);
    expect(timers.pendingCount()).toBe(1);
    c.write(); // 第二读不依赖新的目录通知。
    expect(timers.runNext()).toBe(true);
    expect(JSON.parse(readFileSync(c.responsePath, "utf8"))).toMatchObject({ nonce: c.request.nonce, status: "granted" });
    expect(timers.pendingCount()).toBe(0);

    rmSync(c.requestPath);
    watch.notify("rename", ".request.tmp");
    expect(timers.runNext()).toBe(true);
    expect(timers.pendingCount()).toBe(1);
    expect(timers.runNext()).toBe(true);
    expect(timers.pendingCount()).toBe(0); // 同一事件最多触发一次缺席重读。
  } finally {
    live.dispose();
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;
  }
}));

test("dispose cancels a queued responder read", () => fixture(c => {
  c.responder.dispose();
  const watch = fakeWatch();
  const timers = manualTimers();
  const originalSetTimeout = globalThis.setTimeout;
  const originalClearTimeout = globalThis.clearTimeout;
  let live: ReturnType<typeof createPermissionProbeResponder> | null = null;
  try {
    live = createPermissionProbeResponder({ root: c.root, installId, appPath, pid: 123,
      processStartIdentity: start, platform: "darwin", incognito: false, now: () => 2000,
      systemPreferences: { getMediaAccessStatus: () => "granted" } });
  } finally { watch.restore(); }
  if (!live) throw new Error("watch responder fixture was not created");
  (globalThis as any).setTimeout = timers.setTimeout;
  (globalThis as any).clearTimeout = timers.clearTimeout;
  try {
    c.write();
    watch.notify("rename", ".request.tmp");
    expect(timers.pendingCount()).toBe(1);
    live.dispose();
    expect(timers.pendingCount()).toBe(0);
    expect(timers.runNext()).toBe(false);
    expect(existsSync(c.responsePath)).toBe(false);
  } finally {
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;
  }
}));

test("one-shot responder retires only after the CLI removes a published request", () => fixture(async c => {
  c.responder.dispose();
  const retired: string[] = [];
  let screenCalls = 0;
  const live = createPermissionProbeResponder({ root: c.root, installId, appPath, pid: 123, processStartIdentity: start,
    platform: "darwin", incognito: false, now: () => 2000,
    systemPreferences: { getMediaAccessStatus: () => { screenCalls++; return "granted"; } },
    onRequestRetired: (nonce: string) => retired.push(nonce) });
  if (!live) throw new Error("one-shot responder was not created");
  const waitFor = async (condition: () => boolean) => {
    const deadline = Date.now() + 1500;
    while (!condition() && Date.now() < deadline) await Bun.sleep(10);
    expect(condition()).toBe(true);
  };
  try {
    c.write();
    await waitFor(() => existsSync(c.responsePath));
    expect(retired).toEqual([]);

    c.write({ ...c.request, nonce: "b".repeat(32) });
    await waitFor(() => JSON.parse(readFileSync(c.responsePath, "utf8")).nonce === "b".repeat(32));
    expect(retired).toEqual([]);

    rmSync(c.requestPath);
    await waitFor(() => retired.length > 0);
    expect(retired).toEqual(["b".repeat(32)]);

    c.write({ ...c.request, nonce: "c".repeat(32) });
    await Bun.sleep(50);
    expect(retired).toEqual(["b".repeat(32)]);
    expect(screenCalls).toBe(2);
  } finally { live.dispose(); }
}));

test("ordinary responder remains active when the CLI removes a completed request", () => fixture(async c => {
  c.responder.dispose();
  const live = createPermissionProbeResponder({ root: c.root, installId, appPath, pid: 123, processStartIdentity: start,
    platform: "darwin", incognito: false, now: () => 2000,
    systemPreferences: { getMediaAccessStatus: () => "granted" } });
  if (!live) throw new Error("ordinary responder was not created");
  const waitForSecondResponse = async () => {
    const deadline = Date.now() + 1500;
    while (Date.now() < deadline) {
      if (existsSync(c.responsePath) && JSON.parse(readFileSync(c.responsePath, "utf8")).nonce === "b".repeat(32)) return;
      await Bun.sleep(10);
    }
    throw new Error("ordinary responder did not observe the next request");
  };
  try {
    c.write();
    const firstDeadline = Date.now() + 1500;
    while (!existsSync(c.responsePath) && Date.now() < firstDeadline) await Bun.sleep(10);
    expect(JSON.parse(readFileSync(c.responsePath, "utf8")).nonce).toBe(c.request.nonce);

    rmSync(c.requestPath);
    c.write({ ...c.request, nonce: "b".repeat(32) });
    await waitForSecondResponse();
    expect(JSON.parse(readFileSync(c.responsePath, "utf8")).status).toBe("granted");
  } finally { live.dispose(); }
}));

test("terminal guide requests cannot restart probing or replace a completed decision", () => fixture(c => {
  c.write();
  writeFileSync(join(c.directory, "accessibility-setup.json"), JSON.stringify({ ...c.marker, state: "deferred" }));
  c.responder.respond();
  expect(c.calls()).toBe(0);
  expect(existsSync(c.responsePath)).toBe(false);
}));
