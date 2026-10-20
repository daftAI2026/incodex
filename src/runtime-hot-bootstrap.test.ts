/**
 * [INPUT]: 依赖 RC11 冻结验证器、当前完整候选与固定迁移摘要。
 * [OUTPUT]: 固定显式迁移摘要；验证旧 verifier 拒绝会话销毁原语热换。
 * [POS]: 兼容边界回归；迁移前基线只合成固定资产变化，不证明 App 热激活。
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
// ---- 固定安装迁移锚；新短命卡资产不进入旧 verifier 热换白名单。 ----
const installMigrationDigests: Record<string, string> = {
  "incodex-loader.cjs": "ec85f12f96cedecb29cf1bc63282a428967058e2502e431cdc1e32a393a0570f",
  "incodex-main.cjs": "8f7e45ac93abdcedca0b2420e73a309a6507ec96af6e7408227aa177c7921ded",
  "incodex-permission-host": "3f18041ddb6011a91c96a6c1713cd048ca57977ade794a19c1c71e9ae7756f80",
  "incodex-permission-ui.dylib": "1043596cf891cb8891981dc2cc1374cae0f27ecaf53dfa85745702df1399aeae",
  "runtime-native-manifest.json": "06ea377c443a393565af185064fd9f5a6d42e33940b55d19eda4ad0c6deec17f",
};
const installMigrationAssets = Object.keys(installMigrationDigests).sort();
const restartMigrationDigests: Record<string, string> = {
  "incodex-safe-home.cjs": "b3b5bcaee8612cb66843439c4433cedc32a241b89ab7d9e49b2e9b265b618c06",
};
const fixedDigest = (name: string) => restartMigrationDigests[name] ?? installMigrationDigests[name] ?? frozen.files[name];

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
  // 合成 A 只验证固定资产变化的拒绝。
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

function syntheticPreMigrationSources(sources: Record<string, string | Buffer>) {
  const previous = { ...sources };
  const appendPrior = (name: string) => Buffer.concat([
    Buffer.isBuffer(sources[name]) ? sources[name] : Buffer.from(sources[name]),
    Buffer.from("\n// synthetic pre-migration fixture\n"),
  ]);
  previous["incodex-loader.cjs"] = appendPrior("incodex-loader.cjs");
  previous["incodex-main.cjs"] = appendPrior("incodex-main.cjs");
  previous["incodex-permission-host"] = appendPrior("incodex-permission-host");
  previous["incodex-permission-ui.dylib"] = appendPrior("incodex-permission-ui.dylib");

  const nativeManifest = JSON.parse(String(sources["runtime-native-manifest.json"]));
  nativeManifest.files["incodex-permission-host"] = hash(previous["incodex-permission-host"]);
  nativeManifest.files["incodex-permission-ui.dylib"] = hash(previous["incodex-permission-ui.dylib"]);
  nativeManifest.sourceSha256 = hash(Buffer.from("synthetic pre-migration view sources"));
  nativeManifest.hostSourceSha256 = hash(Buffer.from("synthetic pre-migration host sources"));
  previous["runtime-native-manifest.json"] = Buffer.from(`${JSON.stringify(nativeManifest, null, 2)}\n`);
  return previous;
}

function pairWithCurrentMacOSMigration() {
  const home = mkdtempSync(join(tmpdir(), "incodex-current-migration-"));
  const candidateSources = macOSPublishedSources();
  const previousSources = syntheticPreMigrationSources(candidateSources);
  const a = publish(home, { ...previousSources, [loadName]: oldBytes }, "a".repeat(40));
  const verifier = oldVerifier();
  const initial = verifier.readRendererGeneration(a, { HOME: home });
  const b = publish(home, { ...candidateSources, [loadName]: oldBytes }, "b".repeat(40));
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

test("fixed assets retain RC11 hashes except the explicit install migration", () => {
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

test("readonly cleanup migration stays restart-only with the frozen verifier", async () => {
  const home = mkdtempSync(join(tmpdir(), "incodex-cleanup-migration-"));
  try {
    const sources = currentSources(), name = "incodex-safe-home.cjs";
    const previous = { ...sources, [name]: `${sources[name]}\n// synthetic prior cleanup\n`, [loadName]: oldBytes };
    const a = publish(home, previous, "a".repeat(40)), verifier = oldVerifier();
    const initial = verifier.readRendererGeneration(a, { HOME: home });
    publish(home, { ...sources, [loadName]: oldBytes }, "b".repeat(40));
    const candidate = verifier.prepareRendererUpdate(a, { HOME: home }, process.execPath,
      { mainActions: true, permissionCopy: true });
    expect(hotFiles.has(name)).toBe(false);
    expect(Object.keys(candidate.selection.files).filter(key =>
      candidate.selection.files[key] !== initial.selection.files[key])).toEqual([name]);
    expect(candidate).toMatchObject({ restartRequired: true, installRequired: false, source: "" });
    let applied = 0;
    const coordinator = verifier.createRendererUpdateCoordinator({ initial, prepare: () => candidate,
      windows: () => { throw new Error("fixed cleanup migration must not enumerate live windows"); },
      apply: async () => { applied++; return true; }, isSelected: verifier.rendererUpdateStillSelected });
    await coordinator.refresh();
    expect(coordinator.status().phase).toBe("restart-required");
    expect(coordinator.status().active.key).toBe(initial.key);
    expect(applied).toBe(0);
  } finally { rmSync(home, { recursive: true, force: true }); }
});

test(
  "the single main migration remains restart-only without an install requirement",
  async () => {
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
  },
);

test.skipIf(process.platform !== "darwin")(
  "the complete current install migration requires install and stages no renderer",
  async () => {
    const f = pairWithCurrentMacOSMigration();
    try {
      const candidate = f.next();
      const changed = Object.keys(candidate.selection.files).filter(name =>
        candidate.selection.files[name] !== f.initial.selection.files[name]);
      expect(changed.sort()).toEqual(installMigrationAssets);
      expect(candidate).toMatchObject({ restartRequired: true, installRequired: true, source: "" });
      let windows = 0, applied = 0;
      const coordinator = f.verifier.createRendererUpdateCoordinator({ initial: f.initial,
        prepare: () => candidate, windows: () => { windows++; return ["same-page"]; },
        apply: async () => { applied++; return true; }, isSelected: f.verifier.rendererUpdateStillSelected });
      await coordinator.refresh();
      expect(coordinator.status().phase).toBe("install-required");
      expect(windows).toBe(0);
      expect(applied).toBe(0);
      expect(coordinator.status().active.key).toBe(f.initial.key);
    } finally { rmSync(f.home, { recursive: true, force: true }); }
  },
);

test.skipIf(process.platform !== "darwin")(
  "macOS catalog retains 23 asset names and the pinned install-migration hashes",
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
