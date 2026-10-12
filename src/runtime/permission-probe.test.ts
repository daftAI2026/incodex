/**
 * [INPUT]: 依赖临时私有事务目录与注入的 Electron API。
 * [OUTPUT]: 验证请求身份、screen 自查及单个 responder 的功能代际切换。
 * [POS]: 状态回传失败先行测试；不访问真实 App 或 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import * as fs from "node:fs";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runInNewContext } from "node:vm";
import { createPermissionProbeResponder } from "./incodex-permission-probe.cts";
import { createPermissionProbePolicy, interpretScreenPermission } from "./incodex-permission-policy.cts";

const installId = "11111111-1111-4111-8111-111111111111";
// 与 CLI request_setup 的 16 随机字节小写十六进制编码一致；不是 installId 的 UUID。
const requestId = "22222222222222222222222222222222";
const appPath = "/Applications/ChatGPT.app";
const start = "Mon Oct 12 01:00:00 2026";

test("pure policy interprets a value without receiving any system capability", () => {
  for (const raw of ["granted", "denied", "not-determined", "restricted", "unknown"]) {
    const result = interpretScreenPermission(raw);
    expect(result).toEqual({ rawStatus: raw, status: raw === "granted" ? "granted" : raw === "unknown" ? "unknown" : "not-granted" });
  }
  for (const raw of [undefined, true, {}, "new-system-status"]) {
    expect(interpretScreenPermission(raw)).toEqual({ rawStatus: "unknown", status: "unknown" });
  }
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
    platform: "darwin", incognito: false, policyForRequest: createPermissionProbePolicy, now: () => 2000, watch: false,
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

test("request identity rejects UUIDs and malformed hex even when the CLI marker matches", () => fixture(c => {
  const markerPath = join(c.directory, "accessibility-setup.json");
  for (const invalid of [installId, "A".repeat(32), "2".repeat(31), "g".repeat(32), ""]) {
    writeFileSync(markerPath, JSON.stringify({ ...c.marker, requestId: invalid }));
    c.write({ ...c.request, requestId: invalid }); c.responder.respond();
    expect(c.calls()).toBe(0);
    expect(existsSync(c.responsePath)).toBe(false);
  }
  writeFileSync(markerPath, JSON.stringify(c.marker));
  c.write(); c.responder.respond();
  expect(c.calls()).toBe(1);
  expect(JSON.parse(readFileSync(c.responsePath, "utf8"))).toMatchObject({ requestId, status: "not-granted" });
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
    platform: "darwin", incognito: false, policyForRequest: createPermissionProbePolicy, now: () => 2000, watch: false,
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
  const nextRequestId = "33333333333333333333333333333333";
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
    platform: "darwin", incognito: false, policyForRequest: createPermissionProbePolicy, now: () => 2000,
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

test("hot permission policy receives a value, never the Electron authority object", () => fixture(c => {
  c.responder.dispose();
  const inputs: unknown[] = [], calls: string[] = [];
  const live = createPermissionProbeResponder({ root: c.root, installId, appPath, pid: 123, processStartIdentity: start,
    platform: "darwin", now: () => 2000, watch: false,
    systemPreferences: { getMediaAccessStatus: (kind: string) => { calls.push(kind); return "denied"; },
      askForMediaAccess() { throw new Error("candidate must never see authorization APIs"); } },
    policyForRequest: () => ({ protocol: 1, requestIdValid: () => true,
      interpretScreenPermission(value: unknown) { inputs.push(value); return { rawStatus: "denied", status: "not-granted" }; } }) });
  if (!live) throw new Error("service responder was not created");
  try {
    c.write(); live.respond();
    expect(inputs).toEqual(["denied"]);
    expect(calls).toEqual(["screen"]);
    expect(JSON.parse(readFileSync(c.responsePath, "utf8"))).toMatchObject({ rawStatus: "denied", status: "not-granted" });
  } finally { live.dispose(); }
}));

test("policy cannot replace a denied system observation with a granted declaration", () => fixture(c => {
  c.responder.dispose();
  let calls = 0;
  const live = createPermissionProbeResponder({ root: c.root, installId, appPath, pid: 123, processStartIdentity: start,
    platform: "darwin", now: () => 2000, watch: false,
    systemPreferences: { getMediaAccessStatus: () => { calls++; return "denied"; } },
    policyForRequest: () => ({ protocol: 1, requestIdValid: () => true,
      interpretScreenPermission() { return { rawStatus: "granted", status: "granted" }; } }) });
  if (!live) throw new Error("service responder was not created");
  try {
    c.write(); live.respond();
    expect(existsSync(c.responsePath)).toBe(false);
    expect(calls).toBe(1);
  } finally { live.dispose(); }
}));

test("a policy accessor cannot change the checked decision before publication", () => fixture(c => {
  c.responder.dispose();
  let reads = 0;
  const live = createPermissionProbeResponder({ root: c.root, installId, appPath, pid: 123, processStartIdentity: start,
    platform: "darwin", now: () => 2000, watch: false,
    systemPreferences: { getMediaAccessStatus: () => "denied" },
    policyForRequest: () => ({ protocol: 1, requestIdValid: () => true,
      interpretScreenPermission: () => ({ rawStatus: "denied",
        get status() { return ++reads === 1 ? "not-granted" : "granted"; } }) }) });
  if (!live) throw new Error("service responder was not created");
  try {
    c.write(); live.respond();
    expect(JSON.parse(readFileSync(c.responsePath, "utf8"))).toMatchObject({ rawStatus: "denied", status: "not-granted" });
    expect(reads).toBe(1);
  } finally { live.dispose(); }
}));

test("one responder captures the current service policy without resetting nonce ownership", () => fixture(c => {
  c.responder.dispose();
  const seen: string[] = [];
  const policy = (name: string, accepts: boolean) => ({
    protocol: 1,
    requestIdValid(value: unknown) { seen.push(`${name}:validate`); return accepts && value === requestId; },
    interpretScreenPermission() { seen.push(`${name}:read`); return { rawStatus: "granted", status: "granted" }; },
  });
  let current = policy("A", false);
  const live = createPermissionProbeResponder({ root: c.root, installId, appPath, pid: 123, processStartIdentity: start,
    platform: "darwin", now: () => 2000, watch: false, policyForRequest: () => current,
    systemPreferences: { getMediaAccessStatus: () => "granted" } });
  if (!live) throw new Error("service responder was not created");
  try {
    c.write(); live.respond();
    expect(existsSync(c.responsePath)).toBe(false);
    expect(seen).toEqual(["A:validate"]);
    current = policy("B", true);
    live.respond();
    expect(seen).toEqual(["A:validate", "B:validate", "B:read"]);
    expect(JSON.parse(readFileSync(c.responsePath, "utf8"))).toMatchObject({ ...c.request, status: "granted" });
    current = policy("C", true);
    live.respond();
    // 换代不清空 nonce；不能把旧请求当作新请求再次执行。
    expect(seen).toEqual(["A:validate", "B:validate", "B:read"]);
    c.write({ ...c.request, nonce: "b".repeat(32) }); live.respond();
    expect(seen.slice(-2)).toEqual(["C:validate", "C:read"]);
  } finally { live.dispose(); }
}));

test("a request keeps one policy snapshot when the active generation changes during validation", () => fixture(c => {
  c.responder.dispose();
  const reads: string[] = [];
  const b = { protocol: 1, requestIdValid: () => true,
    interpretScreenPermission() { reads.push("B"); return { rawStatus: "granted", status: "granted" }; } };
  let raw = "denied";
  let current: any = { protocol: 1, requestIdValid() { current = b; return true; },
    interpretScreenPermission() { reads.push("A"); return { rawStatus: "denied", status: "not-granted" }; } };
  const live = createPermissionProbeResponder({ root: c.root, installId, appPath, pid: 123, processStartIdentity: start,
    platform: "darwin", now: () => 2000, watch: false, policyForRequest: () => current,
    systemPreferences: { getMediaAccessStatus: () => raw } });
  if (!live) throw new Error("service responder was not created");
  try {
    c.write(); live.respond();
    expect(reads).toEqual(["A"]);
    expect(JSON.parse(readFileSync(c.responsePath, "utf8")).status).toBe("not-granted");
    raw = "granted"; c.write({ ...c.request, nonce: "b".repeat(32) }); live.respond();
    expect(reads).toEqual(["A", "B"]);
    expect(JSON.parse(readFileSync(c.responsePath, "utf8")).status).toBe("granted");
  } finally { live.dispose(); }
}));

test("a service policy cannot bypass stable install/process/marker authorization", () => fixture(c => {
  c.responder.dispose();
  let reads = 0;
  const live = createPermissionProbeResponder({ root: c.root, installId, appPath, pid: 123, processStartIdentity: start,
    platform: "darwin", now: () => 2000, watch: false, policyForRequest: () => ({ protocol: 1,
      requestIdValid: () => true, interpretScreenPermission() { reads++; return { rawStatus: "granted", status: "granted" }; } }),
    systemPreferences: { getMediaAccessStatus: () => "granted" } });
  if (!live) throw new Error("service responder was not created");
  try {
    for (const change of [{ pid: 124 }, { processStartIdentity: "different" }, { appPath: "/tmp/other.app" }, { requestId: "other" }]) {
      c.write({ ...c.request, ...change }); live.respond();
      expect(existsSync(c.responsePath)).toBe(false);
    }
    expect(reads).toBe(0);
    c.write(); live.respond(); expect(reads).toBe(1);
    expect(JSON.parse(readFileSync(c.responsePath, "utf8")).status).toBe("granted");
  } finally { live.dispose(); }
}));

test("real filesystem events reuse one watcher and nonce owner through A, B and C", () => fixture(async c => {
  c.responder.dispose();
  let watches = 0, closes = 0, current = "A", observations = 0;
  const policies: string[] = [];
  const source = readFileSync(join(import.meta.dir, "incodex-permission-probe.cts"), "utf8")
    .replace("export { createPermissionProbeResponder };", "createPermissionProbeResponder;");
  // 包装真实 fs.watch 计数，不替换其事件或生产接口。
  const observedFs = { ...fs, watch(...args: any[]) {
    watches++;
    const watcher = (fs.watch as any)(...args), close = watcher.close.bind(watcher);
    watcher.close = () => { closes++; return close(); };
    return watcher;
  } };
  const create = runInNewContext(source, { process, Buffer, require: (name: string) => name === "node:fs" ? observedFs : require(name) });
  const live = create({ root: c.root, installId, appPath, pid: 123, processStartIdentity: start,
    platform: "darwin", now: () => 2000,
    systemPreferences: { getMediaAccessStatus: () => { observations++; return "granted"; } },
    policyForRequest: () => {
      const name = current, base = createPermissionProbePolicy();
      return { ...base, interpretScreenPermission(value: string) { policies.push(name); return base.interpretScreenPermission(value); } };
    } });
  expect(live).not.toBeNull();
  try {
    for (const [name, nonce] of [["A", "a"], ["B", "b"], ["C", "c"]]) {
      current = name; c.write({ ...c.request, nonce: nonce.repeat(32) });
      const deadline = Date.now() + 1500;
      while (Date.now() < deadline) {
        if (existsSync(c.responsePath) && JSON.parse(readFileSync(c.responsePath, "utf8")).nonce === nonce.repeat(32)) break;
        await Bun.sleep(10);
      }
      expect(JSON.parse(readFileSync(c.responsePath, "utf8"))).toMatchObject({ nonce: nonce.repeat(32), status: "granted" });
    }
    expect(policies).toEqual(["A", "B", "C"]);
    expect(observations).toBe(3); expect(watches).toBe(1); expect(closes).toBe(0);
    current = "D"; live.respond();
    expect(observations).toBe(3);
  } finally { live.dispose(); live.dispose(); }
  expect(closes).toBe(1);
}));
