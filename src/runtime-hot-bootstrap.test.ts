/**
 * [INPUT]: 冻结 RC11 验证器/清单、当前 dist 与私有发布夹具。
 * [OUTPUT]: 旧验证器接受业务候选，ACK 后提交；固定代码变化仍拒绝。
 * [POS]: 旧宿主升级回归；不替换验证器，不启动 App 或读取 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runInNewContext } from "node:vm";

const root = join(import.meta.dir, "..");
const fixtures = join(root, "crates/incodex-cli/tests/fixtures");
const oldBytes = readFileSync(join(fixtures, "runtime-load-rc11.cjs"));
const frozen = JSON.parse(readFileSync(join(fixtures, "runtime-manifest-rc11.json"), "utf8"));
const catalog = JSON.parse(readFileSync(join(root, "runtime-artifacts.json"), "utf8"));
const hash = (bytes: string | Buffer) => createHash("sha256").update(bytes).digest("hex");
const loadName = "incodex-runtime-load.cjs";
const hotFiles = new Set(["incodex-inject.js", "incodex-main-actions.cjs", "incodex-permission-copy.json"]);

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

function publish(home: string, sources: Record<string, string>, sourceCommit: string) {
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

function pair(changeBootstrap = false) {
  const home = mkdtempSync(join(tmpdir(), "incodex-frozen-bootstrap-"));
  const sources = currentSources();
  const a = publish(home, { ...sources, [loadName]: oldBytes.toString("utf8") }, "a".repeat(40));
  const verifier = oldVerifier();
  const initial = verifier.readRendererGeneration(a, { HOME: home });
  const b = publish(home, { ...sources, [loadName]: sources[loadName] + (changeBootstrap ? "\n// incompatible bootstrap\n" : "") }, "b".repeat(40));
  const next = () => verifier.prepareRendererUpdate(a, { HOME: home }, process.execPath,
    { mainActions: true, permissionCopy: true });
  return { home, verifier, initial, a, b, next };
}

test("permission-only release is accepted by the frozen installed verifier without replacing it", async () => {
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

test("permission business does not change fixed assets from the installed generation", () => {
  for (const name of [...catalog.external, catalog.loader] as string[]) {
    if (hotFiles.has(name)) continue;
    expect(hash(readFileSync(join(root, "dist", name))), name).toBe(frozen.files[name]);
  }
});

test("frozen verifier still rejects a genuine fixed-bootstrap change", () => {
  const f = pair(true);
  try { expect(f.next()).toMatchObject({ restartRequired: true, installRequired: false, source: "" }); }
  finally { rmSync(f.home, { recursive: true, force: true }); }
});
