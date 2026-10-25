/**
 * [INPUT]: 依赖合成 manifest、激活 token、官方 exports 与 N-API adapter。
 * [OUTPUT]: 验证同 adapter 业务换代、稳定包装与脱敏回执。
 * [POS]: 主进程热激活边界回归；不启动 App 或读取真实 Keychain。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { lstatSync, readFileSync } from "node:fs";
import { join } from "node:path";

const digest = (bytes: Buffer | string) => createHash("sha256").update(bytes).digest("hex");
const hotModule = () => import("./incodex-device-key-hot.cts").catch(() => null);

function fixture(adapterSource = "synthetic signed adapter binary"): any {
  const adapterBytes = Buffer.from(adapterSource);
  const manifest = Buffer.from(`${JSON.stringify({
    schemaVersion: 1, platform: "macos", abiVersion: 1, minimumMacOS: "12.0",
    architectures: ["arm64", "x86_64"], sourceSha256: "a".repeat(64),
    files: { "incodex-device-key-adapter.node": digest(adapterBytes) },
  })}\n`);
  const pin = { manifestSha256: digest(manifest), adapterSha256: digest(adapterBytes) };
  const home = "/private/tmp/device-key-hot-fixture";
  const adapterDir = join(home, ".incodex/helpers/macos-device-key-adapter", pin.adapterSha256);
  const officialPath = "/Applications/ChatGPT.app/Contents/Resources/native/remote-control-device-key.node";
  const files = new Map([
    [join(adapterDir, "manifest.json"), manifest],
    [join(adapterDir, "incodex-device-key-adapter.node"), adapterBytes],
    [join(home, ".incodex/macos-device-key/registration.json"), Buffer.from(JSON.stringify({
      schemaVersion: 1, protocolVersion: 1, helperSha256: "b".repeat(64),
    }))],
  ]);
  const modes = new Map([
    [join(adapterDir, "manifest.json"), 0o600],
    [join(adapterDir, "incodex-device-key-adapter.node"), 0o600],
    [join(home, ".incodex/macos-device-key/registration.json"), 0o600],
  ]);
  const directories = new Set([
    join(home, ".incodex"),
    join(home, ".incodex/helpers"),
    join(home, ".incodex/helpers/macos-device-key-adapter"),
    adapterDir,
    join(home, ".incodex/macos-device-key"),
  ]);
  const events: string[] = [];
  const calls: string[] = [];
  const denied = Object.assign(new Error("secret detail"), { errorCode: "remote_control_device_key_unusable" });
  const canceled = Object.assign(new Error("cancel secret detail"), { name: "AbortError" });
  const signingFailures = [denied, canceled];
  const original = {
    getDeviceKeyPublic(keyId: string) { calls.push(`official-public:${keyId}`); return Promise.resolve("official-public"); },
    signDeviceKey(keyId: string, _payload: Buffer) { calls.push(`official-sign:${keyId}`); return Promise.resolve("official-signature"); },
  };
  const cachedOfficialExports = original;
  const adapter = {
    prepareDeviceKeyAccess(...args: unknown[]) { events.push(`prepare:${args.length}`); return true; },
    getDeviceKeyPublic(keyId: string) { events.push(`native-public:${keyId}`); return Promise.resolve({ publicKeySpkiDerBase64: "public-secret" }); },
    signDeviceKey(keyId: string, _payload: Buffer) { events.push(`native-sign:${keyId}`); return Promise.reject(signingFailures.shift()); },
  };
  const runtime = {
    process: { platform: "darwin", pid: 53655, execPath: "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
      resourcesPath: "/Applications/ChatGPT.app/Contents/Resources", getuid: () => process.getuid?.() ?? 0 },
    os: { homedir: () => home },
    path: require("node:path"),
    crypto: require("node:crypto"),
    fs: {
      lstatSync(file: string) {
        if (directories.has(file)) return { isFile: () => false, isDirectory: () => true,
          isSymbolicLink: () => false, uid: process.getuid?.() ?? 0, mode: 0o40700 };
        const bytes = files.get(file);
        return { isFile: () => Boolean(bytes), isDirectory: () => false, isSymbolicLink: () => false,
          size: bytes?.length ?? 0, uid: process.getuid?.() ?? 0, mode: 0o100000 | (modes.get(file) ?? 0o600), nlink: 1,
          dev: 1, ino: file, mtimeMs: 1 };
      },
      readFileSync(file: string) { const value = files.get(file); if (!value) throw new Error("ENOENT"); return value; },
    },
    requireModule(name: string) {
      events.push(`require:${name}`);
      if (name === officialPath) return cachedOfficialExports;
      if (name === join(adapterDir, "incodex-device-key-adapter.node")) return adapter;
      if (name === "./incodex-safe-home.cjs") return { rotateAndAppendLog: (_root: string, line: string) => events.push(`log:${line}`) };
      if (name === "./incodex-instance.cjs") return { targetStateDir: (root: string) => join(root, "targets/test") };
      throw new Error(`unexpected module: ${name}`);
    },
  };
  return { pin, runtime, files, events, calls, original, cachedOfficialExports, adapter, adapterDir,
    officialPath, denied, canceled };
}

test("feature construction is pure; activation verifies and patches the cached official exports once", async () => {
  const mod = await hotModule();
  expect(typeof mod?.createDeviceKeyHotFeature).toBe("function");
  if (!mod) return;
  const f = fixture();
  let environmentLoads = 0;
  const feature = mod.createDeviceKeyHotFeature(f.pin, () => { environmentLoads++; return f.runtime; },
    `activate-device-key:${"a".repeat(64)}`);
  expect(environmentLoads).toBe(0);
  expect(f.events).toEqual([]);
  const originalGet = f.cachedOfficialExports.getDeviceKeyPublic;

  expect(await feature.activate()).toMatchObject({ ok: true, code: "INSTALLED" });
  expect(environmentLoads).toBe(1);
  expect(f.events).toContain("prepare:0");
  expect(f.cachedOfficialExports).toBe(f.original);
  expect(f.cachedOfficialExports.getDeviceKeyPublic).not.toBe(originalGet);
  expect(await f.cachedOfficialExports.getDeviceKeyPublic(`dk_osn_${"A".repeat(43)}`))
    .toMatchObject({ publicKeySpkiDerBase64: "public-secret" });
  let observed: unknown;
  try { await f.cachedOfficialExports.signDeviceKey(`dk_osn_${"A".repeat(43)}`, Buffer.from("secret payload")); }
  catch (error) { observed = error; }
  expect(observed).toBe(f.denied);
  let canceled: unknown;
  try { await f.cachedOfficialExports.signDeviceKey(`dk_osn_${"A".repeat(43)}`, Buffer.from("secret payload")); }
  catch (error) { canceled = error; }
  expect(canceled).toBe(f.canceled);
  expect(f.calls).toEqual([]);
  expect(await f.cachedOfficialExports.getDeviceKeyPublic(`dk_hse_${"B".repeat(43)}`)).toBe("official-public");
  expect(f.calls).toEqual([`official-public:dk_hse_${"B".repeat(43)}`]);
  expect(await feature.activate()).toMatchObject({ ok: true, code: "ALREADY_ACTIVE" });
  expect(f.events.filter(event => event === "prepare:0")).toHaveLength(1);
  const log = f.events.filter(event => event.startsWith("log:")).join("\n");
  expect(log).toContain("device-key-hot-activation");
  expect(log).toContain("device-key-hot-operation");
  expect(log).toContain('"operation":"sign"');
  expect(log).toContain('"errorCode":"remote_control_device_key_unusable"');
  expect(log).toContain('"errorCode":"unknown"');
  expect(log).not.toContain("dk_osn_");
  expect(log).not.toContain("secret payload");
  expect(log).not.toContain("public-secret");
  expect(log).not.toContain("secret detail");
});

test("a compatible content-addressed adapter replaces one stable wrapper without stealing in-flight work", async () => {
  const mod = await hotModule();
  expect(typeof mod?.createDeviceKeyHotFeature).toBe("function");
  if (!mod) return;
  const first = fixture("adapter generation A");
  const second = fixture("adapter generation B");
  const firstAdapter = first.adapter as any;
  let settleOld!: (value: unknown) => void;
  firstAdapter.signDeviceKey = () => new Promise(resolve => { settleOld = resolve; });
  const secondAdapter = second.adapter as any;
  const secondCalls: string[] = [];
  secondAdapter.signDeviceKey = () => { secondCalls.push("new-sign"); return Promise.resolve("new-signature"); };
  secondAdapter.getDeviceKeyPublic = () => { secondCalls.push("new-public"); return Promise.resolve("new-public"); };
  const secondRequire = second.runtime.requireModule;
  second.runtime.requireModule = (name: string) => name === second.officialPath
    ? first.cachedOfficialExports : secondRequire(name);

  const firstFeature = mod.createDeviceKeyHotFeature(first.pin, () => first.runtime,
    `activate-device-key:${"a".repeat(64)}`);
  const secondFeature = mod.createDeviceKeyHotFeature(second.pin, () => second.runtime,
    `activate-device-key:${"b".repeat(64)}`);
  expect(await firstFeature.activate()).toMatchObject({ ok: true, code: "INSTALLED" });
  const stableSign = first.cachedOfficialExports.signDeviceKey;
  const oldRequest = first.cachedOfficialExports.signDeviceKey(`dk_osn_${"A".repeat(43)}`, Buffer.from("old payload"));
  expect(await secondFeature.activate()).toMatchObject({ ok: true, code: "REPLACED" });
  expect(first.cachedOfficialExports.signDeviceKey).toBe(stableSign);
  expect(await first.cachedOfficialExports.getDeviceKeyPublic(`dk_osn_${"B".repeat(43)}`)).toBe("new-public");
  expect(secondCalls).toEqual(["new-public"]);
  settleOld("old-signature");
  expect(await oldRequest).toBe("old-signature");
});

test("same adapter bytes with a new activation token replaces the business binding", async () => {
  const mod = await hotModule();
  expect(typeof mod?.createDeviceKeyHotFeature).toBe("function");
  if (!mod) return;
  const first = fixture("same adapter bytes");
  const second = fixture("same adapter bytes");
  const firstAdapter = first.adapter;
  let settleOld!: (value: unknown) => void;
  firstAdapter.signDeviceKey = () => new Promise(resolve => { settleOld = resolve; });
  const secondCalls: string[] = [];
  second.adapter.signDeviceKey = () => { secondCalls.push("new-sign"); return Promise.resolve("new-signature"); };
  const secondRequire = second.runtime.requireModule;
  second.runtime.requireModule = (name: string) => name === second.officialPath
    ? first.cachedOfficialExports : secondRequire(name);

  const firstToken = `activate-device-key:${"c".repeat(64)}`;
  const nextToken = `activate-device-key:${"d".repeat(64)}`;
  const firstFeature = mod.createDeviceKeyHotFeature(first.pin, () => first.runtime, firstToken);
  const replayFeature = mod.createDeviceKeyHotFeature(first.pin, () => first.runtime, firstToken);
  const nextFeature = mod.createDeviceKeyHotFeature(second.pin, () => second.runtime, nextToken);
  expect(await firstFeature.activate()).toMatchObject({ ok: true, code: "INSTALLED" });
  const stableSign = first.cachedOfficialExports.signDeviceKey;
  const oldRequest = first.cachedOfficialExports.signDeviceKey(`dk_osn_${"A".repeat(43)}`, Buffer.from("old payload"));
  expect(await replayFeature.activate()).toMatchObject({ ok: true, code: "ALREADY_ACTIVE" });
  expect(await nextFeature.activate()).toMatchObject({ ok: true, code: "REPLACED" });
  expect(first.cachedOfficialExports.signDeviceKey).toBe(stableSign);
  expect(await first.cachedOfficialExports.signDeviceKey(`dk_osn_${"B".repeat(43)}`, Buffer.from("new payload")))
    .toBe("new-signature");
  expect(secondCalls).toEqual(["new-sign"]);
  settleOld("old-signature");
  expect(await oldRequest).toBe("old-signature");
});

test("bad pin or an unpatchable official export fails before native preparation", async () => {
  const mod = await hotModule();
  expect(typeof mod?.createDeviceKeyHotFeature).toBe("function");
  if (!mod) return;
  const f = fixture();
  const badPin = mod.createDeviceKeyHotFeature({ ...f.pin, adapterSha256: "f".repeat(64) }, () => f.runtime,
    `activate-device-key:${"e".repeat(64)}`);
  expect(await badPin.activate()).toMatchObject({ ok: false, code: "UNAVAILABLE" });
  expect(f.events.some(event => event.startsWith("prepare:"))).toBe(false);
  expect(f.events).not.toContain(`require:${f.officialPath}`);
  expect(f.events).not.toContain(`require:${join(f.adapterDir, "incodex-device-key-adapter.node")}`);

  const locked = fixture();
  Object.defineProperty(locked.original, "signDeviceKey", { value: locked.original.signDeviceKey, writable: false, configurable: false });
  const unpatchable = mod.createDeviceKeyHotFeature(locked.pin, () => locked.runtime,
    `activate-device-key:${"f".repeat(64)}`);
  expect(await unpatchable.activate()).toMatchObject({ ok: false, code: "UNAVAILABLE" });
  expect(locked.events.some(event => event.startsWith("prepare:"))).toBe(false);
  expect(locked.original.getDeviceKeyPublic).toBe(locked.cachedOfficialExports.getDeviceKeyPublic);
});

const publishedAdapterDirectory = process.env.INCODEX_DEVICE_KEY_ADAPTER_PUBLISHED_DIR ?? "";
test.skipIf(process.platform !== "darwin" || !publishedAdapterDirectory)(
  "Rust-published adapter bytes pass the Runtime consumer pin verifier",
  async () => {
    const mod = await hotModule();
    expect(typeof mod?.createDeviceKeyHotFeature).toBe("function");
    if (!mod) return;
    const manifestPath = join(publishedAdapterDirectory, "manifest.json");
    const adapterPath = join(publishedAdapterDirectory, "incodex-device-key-adapter.node");
    const manifestBytes = readFileSync(manifestPath);
    const adapterBytes = readFileSync(adapterPath);
    const manifestStat = lstatSync(manifestPath), adapterStat = lstatSync(adapterPath);
    expect(manifestStat.isFile() && !manifestStat.isSymbolicLink()).toBe(true);
    expect(adapterStat.isFile() && !adapterStat.isSymbolicLink()).toBe(true);
    expect(manifestStat.mode & 0o777).toBe(0o600);
    expect(adapterStat.mode & 0o777).toBe(0o600);
    const uid = process.getuid?.();
    expect(uid).toBeDefined();
    expect(manifestStat.uid).toBe(uid!);
    expect(adapterStat.uid).toBe(uid!);
    expect(manifestStat.nlink).toBe(1);
    expect(adapterStat.nlink).toBe(1);

    const manifest = JSON.parse(manifestBytes.toString("utf8"));
    const pin = { manifestSha256: digest(manifestBytes), adapterSha256: digest(adapterBytes) };
    expect(manifest.files["incodex-device-key-adapter.node"]).toBe(pin.adapterSha256);
    const builtActions = readFileSync(join(import.meta.dir, "../../dist/incodex-main-actions.cjs"), "utf8");
    expect(builtActions).toContain(pin.manifestSha256);
    expect(builtActions).toContain(pin.adapterSha256);

    const home = join(publishedAdapterDirectory, "../../../..");
    const officialPath = "/Applications/ChatGPT.app/Contents/Resources/native/remote-control-device-key.node";
    const official = {
      getDeviceKeyPublic() { return Promise.resolve("official"); },
      signDeviceKey() { return Promise.resolve("official"); },
    };
    const adapter = { prepareDeviceKeyAccess: () => true,
      getDeviceKeyPublic: () => Promise.resolve({ publicKeySpkiDerBase64: "synthetic" }),
      signDeviceKey: () => Promise.resolve({ signatureDerBase64: "synthetic" }) };
    const loaded: string[] = [];
    const runtime = {
      process: { platform: "darwin", pid: process.pid, execPath: process.execPath,
        resourcesPath: "/Applications/ChatGPT.app/Contents/Resources", getuid: process.getuid },
      os: { homedir: () => home },
      path: require("node:path"), crypto: require("node:crypto"), fs: require("node:fs"),
      requireModule(name: string) {
        loaded.push(name);
        if (name === officialPath) return official;
        if (name === adapterPath) return adapter;
        if (name === "./incodex-safe-home.cjs") return { rotateAndAppendLog() {} };
        if (name === "./incodex-instance.cjs") return { targetStateDir: () => "synthetic-target" };
        throw new Error("unexpected Runtime dependency");
      },
    };
    const feature = mod.createDeviceKeyHotFeature(pin, () => runtime,
      `activate-device-key:${"1".repeat(64)}`);
    expect(await feature.activate()).toMatchObject({ ok: true, code: "INSTALLED" });
    expect(loaded).toContain(adapterPath);
  },
);
