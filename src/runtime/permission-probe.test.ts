/**
 * [INPUT]: 依赖临时私有事务目录与注入的 Electron API。
 * [OUTPUT]: 验证主进程 screen 自查、请求绑定与只读权限边界。
 * [POS]: 状态回传失败先行测试；不访问真实 App 或 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createPermissionProbeResponder, readScreenPermission } from "./incodex-permission-probe.cts";

const installId = "11111111-1111-4111-8111-111111111111";
const requestId = "22222222-2222-4222-8222-222222222222";
const appPath = "/Applications/ChatGPT.app";
const start = "Mon Oct 12 01:00:00 2026";

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
  const write = (value = request) => writeFileSync(requestPath, JSON.stringify(value), { mode: 0o600 });
  try { await run({ root, directory, marker, responder, request, requestPath, responsePath, write, calls: () => calls,
    setRawStatus: (value: string) => { rawStatus = value; } }); }
  finally { responder?.dispose(); rmSync(root, { recursive: true, force: true }); }
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
    { requestId: installId }, { appPath: "/tmp/ChatGPT.app" }, { requestedAtMs: -9999 }, { nonce: "bad" }, { command: "capture" }]) {
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

test("terminal guide requests cannot restart probing or replace a completed decision", () => fixture(c => {
  c.write();
  writeFileSync(join(c.directory, "accessibility-setup.json"), JSON.stringify({ ...c.marker, state: "deferred" }));
  c.responder.respond();
  expect(c.calls()).toBe(0);
  expect(existsSync(c.responsePath)).toBe(false);
}));
