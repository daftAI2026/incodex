/**
 * [INPUT]: 冻结 RC11 验证器/清单、诊断 main 固定锚与 macOS 原生资产。
 * [OUTPUT]: 验证同 bootstrap 热换与显式 main 迁移；固定资产漂移仍拒绝。
 * [POS]: 隔离验证器/协调器回归门，不证明真实 renderer ACK 或 App 热激活。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runInNewContext } from "node:vm";
import { macOSNativeRuntimeFiles } from "./native-runtime-artifacts.ts";

const root = join(import.meta.dir, "..");
const fixtures = join(root, "crates/incodex-cli/tests/fixtures");
const oldBytes = readFileSync(join(fixtures, "runtime-load-rc11.cjs"));
const frozen = JSON.parse(readFileSync(join(fixtures, "runtime-manifest-rc11.json"), "utf8"));
const catalog = JSON.parse(readFileSync(join(root, "runtime-artifacts.json"), "utf8"));
const hash = (bytes: string | Buffer) => createHash("sha256").update(bytes).digest("hex");
const loadName = "incodex-runtime-load.cjs";
const hotFiles = new Set(["incodex-inject.js", "incodex-main-actions.cjs", "incodex-permission-copy.json"]);
// ---- 唯一已审核的诊断启动迁移；不扩展热换白名单。 ----
const diagnosticMainSha256 = "78e19ccb3d1db63067c59cc3d34aff4f1b97bcd9fc0f9653c5e5caf46d96fea5";
const fixedDigest = (name: string) => name === "incodex-main.cjs" ? diagnosticMainSha256 : frozen.files[name];

function oldVerifier() {
  expect(hash(oldBytes)).toBe("c2827f045d92c668faa7f75187859f523110b81b707fb0b2911c64c5ba119bd1");
  const module = { exports: {} as any };
  runInNewContext(oldBytes.toString("utf8"), { module, exports: module.exports, process, Buffer,
    require: (name: string) => name === "./incodex-instance.cjs"
      ? require("./runtime/incodex-instance.cts") : require(name) });
  return module.exports;
}

function currentSources(): Record<string, string> {
  return Object.fromEntries((catalog.external as string[]).map(name =>
    [name, readFileSync(join(root, "dist", name), "utf8")]));
}

function publish(home: string, sources: Record<string, string | Buffer>, sourceCommit: string) {
  const runtime = join(home, ".incodex/runtime");
  const files = Object.fromEntries(Object.entries(sources).map(([name, bytes]) => [name, hash(bytes)]));
  const body = `${JSON.stringify({ runtimeVersion: "1.4.0", sourceCommit, files })}\n`;
  const key = hash(body), release = `releases/1.4.0-${key}`;
  const releaseDir = join(runtime, release);
  mkdirSync(releaseDir, { recursive: true, mode: 0o700 });
  for (const [name, bytes] of Object.entries(sources)) writeFileSync(join(releaseDir, name), bytes);
  writeFileSync(join(releaseDir, "runtime-manifest.json"), body);
  writeFileSync(join(runtime, "current.json"), JSON.stringify({ schemaVersion: 1, version: "1.4.0",
    sourceCommit, release, manifestSha256: key, files }));
  return releaseDir;
}

function macOSPublishedSources(): Record<string, string | Buffer> {
  const sources: Record<string, string | Buffer> = {
    ...currentSources(),
    [catalog.loader]: readFileSync(join(root, "dist", catalog.loader)),
  };
  const shared = {
    runtimeVersion: "1.4.0",
    sourceCommit: "a".repeat(40),
    files: Object.fromEntries(Object.entries(sources).map(([name, bytes]) => [name, hash(bytes)])),
  };
  const native = macOSNativeRuntimeFiles(join(root, "native/macos"), shared, "darwin");
  for (const [name, bytes] of Object.entries(native)) {
    if (name !== "runtime-manifest.json") sources[name] = bytes;
  }
  return sources;
}

function pair(changeBootstrap = false, changeMain = false) {
  const home = mkdtempSync(join(tmpdir(), "incodex-frozen-bootstrap-"));
  const sources = process.platform === "darwin" ? macOSPublishedSources() : currentSources();
  // 合成 A 只验证分类；真实 RC18 字节迁移由候选隔离门另证。
  const aMain = changeMain ? `${sources["incodex-main.cjs"]}\n// synthetic prior main\n` : sources["incodex-main.cjs"];
  const a = publish(home, { ...sources, "incodex-main.cjs": aMain, [loadName]: oldBytes }, "a".repeat(40));
  const verifier = oldVerifier();
  const initial = verifier.readRendererGeneration(a, { HOME: home });
  const currentLoader = String(sources[loadName]);
  const candidateLoader = changeBootstrap ? `${currentLoader}\n// incompatible bootstrap\n` : currentLoader;
  const b = publish(home, { ...sources, [loadName]: candidateLoader }, "b".repeat(40));
  const next = () => verifier.prepareRendererUpdate(a, { HOME: home }, process.execPath,
    { mainActions: true, permissionCopy: true });
  return { home, verifier, initial, a, b, next };
}

test("same-bootstrap permission release is accepted without replacing the frozen verifier", async () => {
  const f = pair();
  try {
    const candidate = f.next();
    expect(candidate.restartRequired).toBe(false);
    expect(candidate.installRequired).toBe(false);
    expect(candidate.key).not.toBe(f.initial.key);
    let acknowledge = (_value: boolean) => {};
    const ack = new Promise<boolean>(resolve => { acknowledge = resolve; });
    const coordinator = f.verifier.createRendererUpdateCoordinator({ initial: f.initial,
      prepare: () => candidate, windows: () => ["same-page"], apply: () => ack,
      isSelected: f.verifier.rendererUpdateStillSelected });
    const flight = coordinator.refresh();
    await Promise.resolve(); await Promise.resolve();
    expect(coordinator.status().active.key).toBe(f.initial.key);
    acknowledge(true); await flight;
    expect(coordinator.status().phase).toBe("active");
    expect(coordinator.status().active.key).toBe(candidate.key);
  } finally { rmSync(f.home, { recursive: true, force: true }); }
});

test("fixed assets retain historical hashes except the pinned diagnostic main migration", () => {
  for (const name of [...catalog.external, catalog.loader] as string[]) {
    if (hotFiles.has(name)) continue;
    expect(hash(readFileSync(join(root, "dist", name))), name).toBe(fixedDigest(name));
  }
});

test("frozen verifier still rejects a genuine fixed-bootstrap change", () => {
  const f = pair(true);
  try { expect(f.next()).toMatchObject({ restartRequired: true, installRequired: false, source: "" }); }
  finally { rmSync(f.home, { recursive: true, force: true }); }
});

test("diagnostic main migration remains restart-only with no renderer staging", async () => {
  const f = pair(false, true);
  try {
    const candidate = f.next();
    const changed = Object.keys(candidate.selection.files).filter(name =>
      candidate.selection.files[name] !== f.initial.selection.files[name]);
    expect(changed).toEqual(["incodex-main.cjs"]);
    expect(candidate).toMatchObject({ restartRequired: true, installRequired: false, source: "" });
    let windows = 0, applied = 0;
    const coordinator = f.verifier.createRendererUpdateCoordinator({ initial: f.initial,
      prepare: () => candidate, windows: () => { windows++; return ["same-page"]; },
      apply: async () => { applied++; return true; }, isSelected: f.verifier.rendererUpdateStillSelected });
    await coordinator.refresh();
    expect(coordinator.status().phase).toBe("restart-required");
    expect(windows).toBe(0);
    expect(applied).toBe(0);
    expect(coordinator.status().active.key).toBe(f.initial.key);
  } finally { rmSync(f.home, { recursive: true, force: true }); }
});

test.skipIf(process.platform !== "darwin")(
  "macOS catalog retains 23 asset names and the pinned fixed-bootstrap hashes",
  () => {
    const sources = macOSPublishedSources();
    const frozenNames = Object.keys(frozen.files).sort();
    const fixedNames = frozenNames.filter(name => !hotFiles.has(name));
    expect(Object.keys(sources).sort()).toEqual(frozenNames);
    for (const name of fixedNames) expect(hash(sources[name]), name).toBe(fixedDigest(name));

    const f = pair();
    try {
      expect(Object.keys(f.initial.selection.files).sort()).toEqual(frozenNames);
      for (const name of fixedNames) {
        expect(f.initial.selection.files[name], name).toBe(fixedDigest(name));
      }
    } finally { rmSync(f.home, { recursive: true, force: true }); }
  },
);

test.skipIf(process.platform !== "darwin")(
  "frozen verifier rejects a changed real macOS native asset",
  () => {
    const home = mkdtempSync(join(tmpdir(), "incodex-frozen-native-drift-"));
    try {
      const original = macOSPublishedSources();
      const sharedNames = new Set([...catalog.external, catalog.loader]);
      const nativeNames = Object.keys(original).filter(name => !sharedNames.has(name)).sort();
      const frozenNativeNames = Object.keys(frozen.files).filter(name => !sharedNames.has(name)).sort();
      expect(nativeNames).toHaveLength(5);
      expect(nativeNames).toEqual(frozenNativeNames);

      const nativeName = nativeNames[0];
      const changedNative = Buffer.from(original[nativeName]);
      changedNative[changedNative.length - 1] ^= 1;
      const candidateSources = { ...original, [nativeName]: changedNative };
      const initial = publish(home, original, "a".repeat(40));
      const verifier = oldVerifier();
      verifier.readRendererGeneration(initial, { HOME: home });
      publish(home, candidateSources, "b".repeat(40));
      expect(verifier.prepareRendererUpdate(initial, { HOME: home }, process.execPath,
        { mainActions: true, permissionCopy: true })).toMatchObject({
        restartRequired: true, installRequired: false, source: "",
      });
    } finally { rmSync(home, { recursive: true, force: true }); }
  },
);
