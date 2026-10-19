/**
 * [INPUT]: 依赖临时私有事务目录、注入权限 API 与 Node 子进程。
 * [OUTPUT]: 验证 preflight、请求绑定、限次重读和事件投递负控。
 * [POS]: permission probe 回归；真实 fs.watch 在 Node 子进程运行，不访问真实 App 或 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { chmodSync, closeSync, existsSync, fsyncSync, mkdirSync, mkdtempSync, openSync, readFileSync, renameSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
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

const NODE_WATCH_PROBE = String.raw`
const fs = require("node:fs"), os = require("node:os"), path = require("node:path"), vm = require("node:vm");

const diag = {
    watchRegistrations: 0,
    callbackCount: 0,
    deliveredCallbackCount: 0,
    callbacks: [],
    errors: [],
    writes: [],
    responses: [],
    retired: [],
    stages: [],
    screenReads: 0
};

const scenario = process.env.INCODEX_PERMISSION_PROBE_SCENARIO, suppress = process.env.INCODEX_PERMISSION_PROBE_SUPPRESS_EVENT === "1";

const installId = "11111111-1111-4111-8111-111111111111", requestId = "22222222-2222-4222-8222-222222222222";

const appPath = "/Applications/ChatGPT.app", processStartIdentity = "node-watch-fixture", originalWatch = fs.watch;

let responder = null, root = null, requestPath = "", responsePath = "";

function response() {
    try {
        return JSON.parse(fs.readFileSync(responsePath, "utf8"));
    } catch {
        return null;
    }
}

function write(nonce) {
    const request = {
        schemaVersion: 1,
        nonce: nonce,
        installId: installId,
        requestId: requestId,
        appPath: appPath,
        pid: process.pid,
        processStartIdentity: processStartIdentity,
        requestedAtMs: 2e3
    };
    const temp = path.join(path.dirname(requestPath), ".request." + nonce + ".tmp"), fd = fs.openSync(temp, "wx", 384);
    try {
        fs.writeFileSync(fd, JSON.stringify(request));
        fs.fsyncSync(fd);
    } finally {
        fs.closeSync(fd);
    }
    fs.renameSync(temp, requestPath);
    diag.writes.push({
        nonce: nonce,
        exists: fs.existsSync(requestPath)
    });
}

function waitFor(name, condition) {
    return new Promise(resolve => {
        const deadline = Date.now() + 1500;
        const timer = setInterval(() => {
            if (condition()) {
                clearInterval(timer);
                diag.stages.push({
                    name: name,
                    ok: true
                });
                resolve(true);
            } else if (Date.now() >= deadline) {
                clearInterval(timer);
                diag.stages.push({
                    name: name,
                    ok: false
                });
                resolve(false);
            }
        }, 10);
    });
}

function finish(success) {
    try {
        responder?.dispose();
    } catch (error) {
        diag.disposeError = String(error).slice(0, 256);
    }
    fs.watch = originalWatch;
    diag.responseAtEnd = response();
    if (root) fs.rmSync(root, {
        recursive: true,
        force: true
    });
    process.stdout.write(JSON.stringify(diag) + "\n");
    if (!success) process.exitCode = 1;
}

async function run() {
    try {
        const source = fs.readFileSync(process.env.INCODEX_PERMISSION_PROBE_SOURCE, "utf8").replace(/^export \{ ([^\r\n]+) \};?\s*$/m, "module.exports = { $1 };");
        const moduleRecord = {
            exports: {}
        };
        vm.runInNewContext(source, {
            require: require,
            process: process,
            Buffer: Buffer,
            module: moduleRecord,
            exports: moduleRecord.exports,
            setTimeout: setTimeout,
            clearTimeout: clearTimeout,
            console: console
        });
        const create = moduleRecord.exports.createPermissionProbeResponder;
        root = fs.mkdtempSync(path.join(os.tmpdir(), "incodex-permission-probe-node-"));
        const directory = path.join(root, "transactions", installId);
        fs.mkdirSync(directory, {
            recursive: true,
            mode: 448
        });
        fs.chmodSync(path.join(root, "transactions"), 448);
        fs.writeFileSync(path.join(directory, "accessibility-setup.json"), JSON.stringify({
            schemaVersion: 1,
            installId: installId,
            requestId: requestId,
            appPath: appPath,
            presentationOwner: "cli",
            state: "pending",
            requestedAtMs: 1e3
        }), {
            mode: 384
        });
        requestPath = path.join(directory, "permission-probe-request.json");
        responsePath = path.join(directory, "permission-probe-response.json");
        fs.watch = (...args) => {
            diag.watchRegistrations++;
            const callback = args[args.length - 1];
            args[args.length - 1] = (event, file) => {
                diag.callbackCount++;
                if (diag.callbacks.length < 32) diag.callbacks.push({
                    event: String(event).slice(0, 32),
                    filename: file === null ? null : String(file).slice(0, 128)
                });
                if (suppress) return;
                diag.deliveredCallbackCount++;
                callback(event, file);
            };
            const watcher = originalWatch.apply(fs, args);
            watcher.on("error", error => {
                if (diag.errors.length < 4) diag.errors.push({
                    code: error?.code || null,
                    name: error?.name || null
                });
            });
            return watcher;
        };
        responder = create({
            root: root,
            installId: installId,
            requestId: requestId,
            appPath: appPath,
            pid: process.pid,
            processStartIdentity: processStartIdentity,
            platform: "darwin",
            incognito: false,
            now: () => 2e3,
            systemPreferences: {
                getMediaAccessStatus() {
                    diag.screenReads++;
                    return "granted";
                }
            },
            ...scenario === "one-shot" ? {
                onRequestRetired: nonce => diag.retired.push(nonce)
            } : {}
        });
        diag.retirementCallbackConfigured = scenario === "one-shot";
        if (!responder) throw new Error("Node responder was not created");
        fs.watch = originalWatch;
        let success = false;
        if (scenario === "event" || scenario === "negative") {
            write("a".repeat(32));
            success = await waitFor("response-a", () => response()?.nonce === "a".repeat(32));
            diag.requestExistsAfterWrite = diag.writes[0]?.exists === true;
            diag.responseExistsAtDeadline = Boolean(response());
            diag.responses.push(response());
            success = success && diag.screenReads === 1;
        } else if (scenario === "one-shot") {
            write("a".repeat(32));
            const first = await waitFor("response-a", () => response()?.nonce === "a".repeat(32));
            diag.responses.push(response());
            write("b".repeat(32));
            const second = await waitFor("response-b", () => response()?.nonce === "b".repeat(32));
            diag.responses.push(response());
            fs.rmSync(requestPath);
            const retired = await waitFor("retired-b", () => diag.retired.length > 0);
            write("c".repeat(32));
            await new Promise(resolve => setTimeout(resolve, 50));
            diag.responseAtEnd = response();
            success = first && second && retired && diag.retired.length === 1 && diag.retired[0] === "b".repeat(32) && diag.responses[0]?.status === "granted" && diag.responses[1]?.status === "granted" && diag.responses[0]?.nonce === "a".repeat(32) && diag.responses[1]?.nonce === "b".repeat(32) && diag.responseAtEnd?.nonce === "b".repeat(32) && diag.screenReads === 2;
        } else if (scenario === "ordinary") {
            write("a".repeat(32));
            const first = await waitFor("response-a", () => response()?.nonce === "a".repeat(32));
            diag.responses.push(response());
            fs.rmSync(requestPath);
            write("b".repeat(32));
            const second = await waitFor("response-b", () => response()?.nonce === "b".repeat(32));
            diag.responses.push(response());
            success = first && second && !diag.retirementCallbackConfigured && diag.retired.length === 0 && diag.responses[0]?.nonce === "a".repeat(32) && diag.responses[1]?.nonce === "b".repeat(32) && diag.responses.every(value => value?.rawStatus === "granted" && value?.status === "granted") && diag.screenReads === 2;
        } else throw new Error("unknown Node probe scenario");
        if (scenario === "negative") success = false;
        if (diag.watchRegistrations !== 1 || !suppress && diag.deliveredCallbackCount === 0) success = false;
        finish(success);
    } catch (error) {
        diag.error = String(error).slice(0, 512);
        finish(false);
    }
}

void run();
`;

function runNodeWatchProbe(scenario: "event" | "negative" | "one-shot" | "ordinary") {
  const result = spawnSync("node", ["-e", NODE_WATCH_PROBE], {
    encoding: "utf8",
    timeout: 8_000,
    maxBuffer: 16 * 1024,
    env: {
      ...process.env,
      INCODEX_PERMISSION_PROBE_SOURCE: fileURLToPath(new URL("./incodex-permission-probe.cts", import.meta.url)),
      INCODEX_PERMISSION_PROBE_SCENARIO: scenario,
      INCODEX_PERMISSION_PROBE_SUPPRESS_EVENT: scenario === "negative" ? "1" : "0",
    },
  });
  const output = result.stdout.trim().split(/\r?\n/).at(-1) ?? "";
  let diagnostics: Record<string, any> = {};
  try { diagnostics = JSON.parse(output); } catch { /* 下方附有界原始输出。 */ }
  return {
    status: result.status,
    signal: result.signal,
    error: result.error?.message,
    diagnostics,
    output: output.slice(0, 2048),
    stderr: result.stderr.slice(0, 1024),
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

test("the Node event-driven responder observes new requests within the original deadline", () => {
  const result = runNodeWatchProbe("event");
  expect(result.error).toBeUndefined();
  if (result.status !== 0) throw new Error(`Node watch probe failed: ${JSON.stringify(result)}`);
  expect(result.status).toBe(0);
  expect(result.diagnostics).toMatchObject({
    watchRegistrations: 1,
    requestExistsAfterWrite: true,
    responseExistsAtDeadline: true,
    screenReads: 1,
    responses: [{ nonce: "a".repeat(32), rawStatus: "granted", status: "granted" }],
  });
  expect(result.diagnostics.deliveredCallbackCount).toBeGreaterThan(0);
});

test("the Node event harness fails when its watcher delivery is deliberately withheld", () => {
  const result = runNodeWatchProbe("negative");
  expect(result.error).toBeUndefined();
  if (result.status !== 1) {
    throw new Error(`withheld-event negative control did not fail: ${JSON.stringify(result)}`);
  }
  expect(result.status).toBe(1);
  expect(result.diagnostics).toMatchObject({
    watchRegistrations: 1,
    requestExistsAfterWrite: true,
    responseExistsAtDeadline: false,
    deliveredCallbackCount: 0,
    screenReads: 0,
    responses: [null],
  });
  expect(result.diagnostics.callbackCount).toBeGreaterThan(0);
});

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

test("one-shot responder retires only after the CLI removes a published request", () => {
  const result = runNodeWatchProbe("one-shot");
  if (result.status !== 0) throw new Error(`one-shot Node watch probe failed: ${JSON.stringify(result)}`);
  expect(result.diagnostics).toMatchObject({
    watchRegistrations: 1,
    retirementCallbackConfigured: true,
    writes: [
      { nonce: "a".repeat(32), exists: true },
      { nonce: "b".repeat(32), exists: true },
      { nonce: "c".repeat(32), exists: true },
    ],
    responses: [
      { nonce: "a".repeat(32), rawStatus: "granted", status: "granted" },
      { nonce: "b".repeat(32), rawStatus: "granted", status: "granted" },
    ],
    retired: ["b".repeat(32)],
    responseAtEnd: { nonce: "b".repeat(32), status: "granted" },
    screenReads: 2,
  });
});

test("ordinary responder remains active when the CLI removes a completed request", () => {
  const result = runNodeWatchProbe("ordinary");
  if (result.status !== 0) throw new Error(`ordinary Node watch probe failed: ${JSON.stringify(result)}`);
  expect(result.diagnostics).toMatchObject({
    watchRegistrations: 1,
    retirementCallbackConfigured: false,
    writes: [
      { nonce: "a".repeat(32), exists: true },
      { nonce: "b".repeat(32), exists: true },
    ],
    responses: [
      { nonce: "a".repeat(32), rawStatus: "granted", status: "granted" },
      { nonce: "b".repeat(32), rawStatus: "granted", status: "granted" },
    ],
    retired: [],
    screenReads: 2,
  });
});

test("terminal guide requests cannot restart probing or replace a completed decision", () => fixture(c => {
  c.write();
  writeFileSync(join(c.directory, "accessibility-setup.json"), JSON.stringify({ ...c.marker, state: "deferred" }));
  c.responder.respond();
  expect(c.calls()).toBe(0);
  expect(existsSync(c.responsePath)).toBe(false);
}));
