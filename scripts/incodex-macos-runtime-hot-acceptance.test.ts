/**
 * [INPUT]: 依赖热验收合同和生产 createMainActions 动作工厂。
 * [OUTPUT]: 验证安全门、真实结果归一化、Runtime ACK 和生产会话目录布局。
 * [POS]: 纯单元回归；生产会话夹具只写入临时目录，不启动 App 或读取凭据。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { chmodSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { createMainActions } from "../src/runtime/incodex-main-actions.cts";
import { currentOwner } from "../src/runtime/incodex-instance.cts";
import { createSessionHome } from "../src/runtime/incodex-safe-home.cts";
import {
  acceptedRuntimeAck,
  assertPairEvidence,
  isRuntimeArtifactName,
  matchesExternalRuntimeManifest,
  parseArguments,
  requireBundledIdentity,
  TARGET_ID,
  usage,
  verifyActionResult,
  verifyCliVersion,
} from "./incodex-macos-runtime-hot-acceptance-contract.ts";
import {
  assertStableMainPageTarget,
  foreignOfficialHostPids,
  exactPidFrontmostAppleScriptArgs,
  isolatedHostSpawnOptions,
  currentSessionIdsAt,
  parseSessionOwnerBinding,
  requireNoExistingPrivateSessionAt,
  requireNoOwnerRecordsExceptStaleCanonical,
} from "./incodex-macos-runtime-hot-acceptance-host.ts";
import {
  addedSessionIds,
  requireSessionBaselineUnchangedAt,
  summarizeSessionBaseline,
} from "./incodex-macos-runtime-hot-acceptance-sessions.ts";

const runtimeCatalog = JSON.parse(readFileSync(new URL("../runtime-artifacts.json", import.meta.url), "utf8")) as {
  loader: string;
  external: string[];
};
const nativeRuntimeAssets = [
  "incodex-permission-host",
  "incodex-permission-ui.dylib",
  "incodex-remote-key-compat.node",
  "incodex-remote-key-manifest.json",
  "runtime-native-manifest.json",
];
const sha = (n: string) => n.repeat(64).slice(0, 64);
const assetSha = (name: string) => createHash("sha256").update(name).digest("hex");

function runtimeFiles(action = sha("e")) {
  return Object.fromEntries([...runtimeCatalog.external, ...nativeRuntimeAssets].map((name) => [
    name,
    name === "incodex-main-actions.cjs" ? action : assetSha(name),
  ]));
}

function candidate(overrides: Record<string, unknown> = {}) {
  return {
    schemaVersion: 1,
    candidateId: "candidate-a",
    cliVersion: "1.4.0",
    cliSha256: sha("a"),
    runtimeVersion: "1.4.0",
    runtimeManifestSha256: sha("b"),
    runtimeSourceManifestSha256: sha("c"),
    runtimeFiles: runtimeFiles(),
    ...overrides,
  };
}

function pair() {
  return {
    a: candidate(),
    b: candidate({
      candidateId: "candidate-b",
      cliSha256: sha("1"),
      runtimeManifestSha256: sha("2"),
      runtimeSourceManifestSha256: sha("3"),
      runtimeFiles: runtimeFiles(sha("4")),
      hotAcceptance: {
        kind: "bound-open-denial",
        action: "open",
        artifact: "incodex-main-actions.cjs",
        code: "HOT-ACCEPTANCE-B",
        reason: "hot-acceptance-b",
        artifactSha256: sha("4"),
      },
    }),
  };
}

function withOwnerState(run: (stateRoot: string) => void): void {
  const stateRoot = mkdtempSync(join(tmpdir(), "incodex-hot-owner-"));
  chmodSync(stateRoot, 0o700);
  try { run(stateRoot); }
  finally { rmSync(stateRoot, { recursive: true, force: true }); }
}

function writeOwnerRecord(stateRoot: string, name: string, owner: unknown): Buffer {
  const bytes = Buffer.from(`${JSON.stringify(owner)}\n`);
  const file = join(stateRoot, name);
  writeFileSync(file, bytes, { mode: 0o600 });
  chmodSync(file, 0o600);
  return bytes;
}

function withUserRoot(run: (userRoot: string) => void): void {
  const temporary = mkdtempSync(join(tmpdir(), "incodex-hot-session-layout-"));
  const userRoot = join(temporary, ".incodex");
  mkdirSync(userRoot, { mode: 0o700 });
  try { run(userRoot); }
  finally { rmSync(temporary, { recursive: true, force: true }); }
}

describe("macOS Runtime hot acceptance guards", () => {
  test("keeps L3 contracts on the runner, helpers, and pure tests", () => {
    for (const file of [
      "incodex-macos-runtime-hot-acceptance.ts",
      "incodex-macos-runtime-hot-acceptance-contract.ts",
      "incodex-macos-runtime-hot-acceptance-host.ts",
      "incodex-macos-runtime-hot-acceptance-sessions.ts",
      "incodex-macos-runtime-hot-acceptance.test.ts",
    ]) {
      const source = readFileSync(new URL(`./${file}`, import.meta.url), "utf8");
      const header = source.slice(0, source.indexOf("*/") + 2);
      expect(header, `${file} needs a complete L3 contract`).toContain("[INPUT]:");
      expect(header, `${file} needs a complete L3 contract`).toContain("[OUTPUT]:");
      expect(header, `${file} needs a complete L3 contract`).toContain("[POS]:");
      expect(header, `${file} needs the fixed GEB protocol`).toContain("[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md");
    }
    const hostSource = readFileSync(new URL("./incodex-macos-runtime-hot-acceptance-host.ts", import.meta.url), "utf8");
    expect(hostSource.includes("readOwnerRecords(")).toBe(false);
  });

  test("states that permission readiness is an operator precondition, not a TCC probe", () => {
    expect(usage()).toContain("does not query TCC");
  });

  test("does not treat a recycled owned PID as the same official host", () => {
    const starts = new Map([[41, "owner-start"], [42, "old-start"]]);
    const observed = new Map([[41, "owner-start"], [42, "new-start"], [43, null]]);
    expect(foreignOfficialHostPids([41, 42, 43], starts, (pid) => observed.get(pid) ?? null)).toEqual([42, 43]);
  });

  test("keeps the isolated host outside the acceptance process group", () => {
    const env = { HOME: "/Users/fixture", CODEX_HOME: "/private/tmp/codex-home" };
    expect(isolatedHostSpawnOptions(env)).toEqual({ detached: true, stdio: "ignore", env });
    const source = readFileSync(new URL("./incodex-macos-runtime-hot-acceptance.ts", import.meta.url), "utf8");
    expect(source).toContain("host.unref();");
  });

  test("activates only the exact PID through System Events AppleScript arguments", () => {
    expect(exactPidFrontmostAppleScriptArgs(6123)).toEqual([
      "-e",
      "tell application \"System Events\" to set frontmost of (first process whose unix id is 6123) to true",
    ]);
    expect(() => exactPidFrontmostAppleScriptArgs(0)).toThrow("positive PID");
    expect(() => exactPidFrontmostAppleScriptArgs(1.5)).toThrow("positive PID");
  });

  test("keeps the original CDP target while allowing other pages", () => {
    const original = {
      id: "main-target", type: "page", url: "app://codex/", webSocketDebuggerUrl: "ws://127.0.0.1/main",
    };
    const auxiliary = {
      id: "other-target", type: "page", url: "app://codex/other", webSocketDebuggerUrl: "ws://127.0.0.1/other",
    };
    expect(() => assertStableMainPageTarget([original, auxiliary], original)).not.toThrow();
    expect(() => assertStableMainPageTarget([auxiliary], original)).toThrow("main renderer target changed");
    expect(() => assertStableMainPageTarget([original, { ...original }], original)).toThrow("main renderer target changed");
  });

  test("allows only a proven-dead canonical diagnostic and preserves its bytes", () => {
    withOwnerState((stateRoot) => {
      const before = writeOwnerRecord(stateRoot, "incognito.lock", {
        pid: 2_147_483_647,
        processStartIdentity: "Mon Oct 12 10:00:00 2026",
        execIdentity: "ChatGPT",
        token: "0123456789abcdef0123456789abcdef",
      });
      expect(() => requireNoOwnerRecordsExceptStaleCanonical(stateRoot)).not.toThrow();
      expect(readFileSync(join(stateRoot, "incognito.lock")).equals(before)).toBe(true);
    });
  });

  test("refuses every sidecar, quarantine, takeover claim, and legacy socket", () => {
    for (const name of [
      "incognito.lock.active.0123456789abcdef0123456789abcdef",
      ".incognito.lock.quarantine.fixture",
      ".incognito.lock.takeover",
      "incognito.sock",
    ]) {
      withOwnerState((stateRoot) => {
        writeOwnerRecord(stateRoot, "incognito.lock", {
          pid: 2_147_483_647,
          processStartIdentity: "Mon Oct 12 10:00:00 2026",
          execIdentity: "ChatGPT",
          token: "0123456789abcdef0123456789abcdef",
        });
        const sidecar = join(stateRoot, name);
        writeFileSync(sidecar, "fixture", { mode: 0o600 });
        expect(() => requireNoOwnerRecordsExceptStaleCanonical(stateRoot)).toThrow("owner evidence blocks");
        expect(readFileSync(sidecar, "utf8")).toBe("fixture");
      });
    }
  });

  test("refuses live and unverifiable canonical records", () => {
    withOwnerState((stateRoot) => {
      const liveOwner = currentOwner("fixture", process.execPath);
      writeOwnerRecord(stateRoot, "incognito.lock", liveOwner);
      expect(() => requireNoOwnerRecordsExceptStaleCanonical(stateRoot)).toThrow("owner evidence blocks");
    });
    withOwnerState((stateRoot) => {
      writeOwnerRecord(stateRoot, "incognito.lock", {
        pid: process.pid,
        execIdentity: basename(process.execPath),
        token: "0123456789abcdef0123456789abcdef",
      });
      expect(() => requireNoOwnerRecordsExceptStaleCanonical(stateRoot)).toThrow("owner evidence blocks");
    });
    withOwnerState((stateRoot) => {
      writeOwnerRecord(stateRoot, "incognito.lock", {
        pid: 2_147_483_647,
        processStartIdentity: "unknown",
        execIdentity: "ChatGPT",
        token: "0123456789abcdef0123456789abcdef",
      });
      expect(() => requireNoOwnerRecordsExceptStaleCanonical(stateRoot)).toThrow("owner evidence blocks");
    });
  });

  test("uses the production target-scoped session layout without scanning another target", () => {
    const targetId = TARGET_ID;
    const otherTargetId = TARGET_ID === "abcdef012345" ? "0123456789ab" : "abcdef012345";
    withUserRoot((userRoot) => {
      const other = createSessionHome(userRoot, { targetId: otherTargetId, pid: process.pid });
      expect(currentSessionIdsAt(userRoot, targetId)).toEqual(new Set());
      const emptyBaseline = requireNoExistingPrivateSessionAt(userRoot, targetId, "");
      expect(summarizeSessionBaseline(emptyBaseline).count).toBe(0);

      const target = createSessionHome(userRoot, { targetId, pid: process.pid });
      expect(currentSessionIdsAt(userRoot, targetId)).toEqual(new Set([target.sessionId]));
      expect(() => requireNoExistingPrivateSessionAt(userRoot, targetId, "")).toThrow("owner evidence blocks");
      expect(currentSessionIdsAt(userRoot, otherTargetId)).toEqual(new Set([other.sessionId]));

      const owner = JSON.parse(readFileSync(join(target.root, "owner.json"), "utf8"));
      const directory = lstatSync(target.root);
      expect(parseSessionOwnerBinding(owner, target.sessionId, targetId, directory).pid).toBe(process.pid);
      expect(() => parseSessionOwnerBinding(owner, target.sessionId, otherTargetId, directory)).toThrow("exact owner binding");
      expect(() => parseSessionOwnerBinding(owner, target.sessionId, targetId, { ...directory, ino: directory.ino + 1 })).toThrow("exact owner binding");
    });
  });

  test("preserves an ownerless no-proof session as a readonly baseline and hashes contents", () => {
    withUserRoot((userRoot) => {
      const target = createSessionHome(userRoot, { targetId: "0123456789ab", pid: process.pid });
      const ownerPath = join(target.root, "owner.json");
      rmSync(ownerPath);
      const fixtureValue = "private-session-fixture-must-not-escape";
      writeFileSync(join(target.root, "codex-home", "baseline.txt"), fixtureValue, { mode: 0o600 });

      const baseline = requireNoExistingPrivateSessionAt(userRoot, "0123456789ab", "");
      const summary = summarizeSessionBaseline(baseline);
      expect(summary).toEqual({ count: 1, digest: expect.stringMatching(/^[a-f0-9]{64}$/u) });
      expect(summary.digest).not.toContain(target.sessionId);
      expect(summary.digest).not.toContain(fixtureValue);
      expect(currentSessionIdsAt(userRoot, "0123456789ab")).toEqual(new Set([target.sessionId]));
      expect(addedSessionIds(new Set(baseline.keys()), currentSessionIdsAt(userRoot, "0123456789ab"))).toEqual([]);
      expect(() => requireSessionBaselineUnchangedAt(userRoot, "0123456789ab", baseline, "")).not.toThrow();

      writeFileSync(join(target.root, "codex-home", "baseline.txt"), "changed-private-session-fixture", { mode: 0o600 });
      expect(() => requireSessionBaselineUnchangedAt(userRoot, "0123456789ab", baseline, "")).toThrow("baseline changed");
    });
  });

  test("tracks only new valid sessions beyond the ownerless baseline", () => {
    withUserRoot((userRoot) => {
      const old = createSessionHome(userRoot, { targetId: "0123456789ab", pid: process.pid });
      rmSync(join(old.root, "owner.json"));
      const baseline = requireNoExistingPrivateSessionAt(userRoot, "0123456789ab", "");
      const added = createSessionHome(userRoot, { targetId: "0123456789ab", pid: process.pid });
      expect(addedSessionIds(new Set(baseline.keys()), currentSessionIdsAt(userRoot, "0123456789ab"))).toEqual([added.sessionId]);
    });
  });

  test("refuses owner, burn proof, live references, symlinks, and unknown target entries", () => {
    withUserRoot((userRoot) => {
      const owner = createSessionHome(userRoot, { targetId: "0123456789ab", pid: process.pid });
      expect(() => requireNoExistingPrivateSessionAt(userRoot, "0123456789ab", "")).toThrow("owner evidence blocks");
      rmSync(join(owner.root, "owner.json"));
      const sessionsRoot = join(userRoot, "sessions");
      writeFileSync(join(sessionsRoot, `.incodex-burned-${owner.sessionId}.json`), "fixture", { mode: 0o600 });
      expect(() => requireNoExistingPrivateSessionAt(userRoot, "0123456789ab", "")).toThrow("burn proof blocks");
    });
    withUserRoot((userRoot) => {
      const target = createSessionHome(userRoot, { targetId: "0123456789ab", pid: process.pid });
      rmSync(join(target.root, "owner.json"));
      const reference = `pid=91 command INCODEX_SESSION_ROOT=${target.root}`;
      expect(() => requireNoExistingPrivateSessionAt(userRoot, "0123456789ab", reference)).toThrow("process reference blocks");
    });
    withUserRoot((userRoot) => {
      const targetRoot = join(userRoot, "sessions", "0123456789ab");
      const outside = join(userRoot, "outside");
      mkdirSync(outside, { mode: 0o700 });
      mkdirSync(targetRoot, { recursive: true, mode: 0o700 });
      symlinkSync(outside, join(targetRoot, "s-link"));
      expect(() => requireNoExistingPrivateSessionAt(userRoot, "0123456789ab", "")).toThrow("target session entry blocks");
    });
    withUserRoot((userRoot) => {
      const targetRoot = join(userRoot, "sessions", "0123456789ab");
      mkdirSync(targetRoot, { recursive: true, mode: 0o700 });
      writeFileSync(join(targetRoot, "unexpected"), "fixture", { mode: 0o600 });
      expect(() => requireNoExistingPrivateSessionAt(userRoot, "0123456789ab", "")).toThrow("target session entry blocks");
      expect(() => currentSessionIdsAt(userRoot, "0123456789ab")).toThrow("unknown target session entry");
    });
  });

  test("does not follow a target-session symlink or accept a malformed target id", () => {
    withUserRoot((userRoot) => {
      const sessionsRoot = join(userRoot, "sessions");
      const external = join(userRoot, "external-sessions");
      mkdirSync(sessionsRoot, { mode: 0o700 });
      mkdirSync(external, { mode: 0o700 });
      symlinkSync(external, join(sessionsRoot, TARGET_ID));
      expect(() => requireNoExistingPrivateSessionAt(userRoot, TARGET_ID)).toThrow("target session root is not a real directory");
      expect(() => currentSessionIdsAt(userRoot, TARGET_ID)).toThrow("target session root is not a real directory");
      expect(() => requireNoExistingPrivateSessionAt(userRoot, "../other-target")).toThrow("target session identity is invalid");
    });
  });

  test("refuses to run unless --run is explicit and all frozen inputs are named", () => {
    expect(() => parseArguments([])).toThrow("--run");
    expect(() => parseArguments(["--a-cli", "/tmp/a"])).toThrow("--run");
    expect(() => parseArguments([
      "--run",
      "--a-cli", "/tmp/a",
      "--a-evidence", "/tmp/a.json",
      "--b-cli", "/tmp/b",
      "--b-evidence", "/tmp/b.json",
      "--out", "/tmp/out",
    ])).toThrow("/private/tmp");
  });

  test("accepts only same-version candidates with a single production action difference", () => {
    expect(assertPairEvidence(pair())).toEqual({
      aId: "candidate-a",
      bId: "candidate-b",
      action: "open",
      code: "HOT-ACCEPTANCE-B",
      reason: "hot-acceptance-b",
    });
  });

  test("rejects an unready B manifest or an unrelated Runtime change", () => {
    const noBProof = pair();
    delete (noBProof.b as Record<string, unknown>).hotAcceptance;
    expect(() => assertPairEvidence(noBProof)).toThrow("B action proof");

    const changedInjector = pair();
    (changedInjector.b as any).runtimeFiles["incodex-inject.js"] = sha("9");
    expect(() => assertPairEvidence(changedInjector)).toThrow("may differ only in incodex-main-actions.cjs");
  });

  test("accepts the exact native runtime manifest asset from frozen candidates", () => {
    expect(assertPairEvidence(pair()).bId).toBe("candidate-b");
    expect(isRuntimeArtifactName("runtime-native-manifest.json")).toBe(true);
    expect(isRuntimeArtifactName("runtime-native-manifest-copy.json")).toBe(false);

    const unsupported = pair();
    delete (unsupported.a as any).runtimeFiles["runtime-native-manifest.json"];
    (unsupported.a as any).runtimeFiles["runtime-native-manifest-copy.json"] = sha("8");
    expect(() => assertPairEvidence(unsupported)).toThrow("unsupported Runtime artifact name");
  });

  test("reads the embedded Runtime identity from status.externalRuntime", () => {
    const evidence = candidate();
    expect(() => requireBundledIdentity({
      externalRuntime: {
        bundledVersion: evidence.runtimeVersion,
        bundledManifestSha256: evidence.runtimeManifestSha256,
      },
    }, evidence, "A")).not.toThrow();
    expect(() => requireBundledIdentity({
      checks: { runtime: { bundledVersion: evidence.runtimeVersion } },
    } as any, evidence, "A")).toThrow("embedded Runtime");
  });

  test("omits only the catalogued in-bundle loader from the external pointer map", () => {
    const externalFiles = { "incodex-main.cjs": sha("a"), "runtime-native-manifest.json": sha("b") };
    const manifestFiles = { ...externalFiles, [runtimeCatalog.loader]: sha("c") };
    expect(matchesExternalRuntimeManifest(externalFiles, manifestFiles)).toBe(true);
    expect(matchesExternalRuntimeManifest({ ...externalFiles, [runtimeCatalog.loader]: sha("c") }, manifestFiles)).toBe(false);
    expect(matchesExternalRuntimeManifest(externalFiles, { ...manifestFiles, "incodex-unlisted.cjs": sha("d") })).toBe(false);
  });

  test("rejects candidate labels that could inject paths or control data into the report", () => {
    const unsafe = pair();
    (unsafe.a as Record<string, unknown>).candidateId = "../private-token";
    expect(() => assertPairEvidence(unsafe)).toThrow("A frozen evidence is incomplete");
  });

  test("acknowledges only the expected live generation in the same process and window", () => {
    const ack = {
      phase: "active",
      pid: 41,
      processStartIdentity: "Mon Oct 12 10:00:00 2026",
      published: { generation: sha("a") },
      main: { generation: sha("a"), artifact: sha("b") },
      renderers: [{ windowId: 7, generation: sha("a"), ui: sha("c"), state: "acknowledged" }],
      restartRequired: false,
      installRequired: false,
      rollbackFailed: false,
    };
    expect(acceptedRuntimeAck(ack, {
      pid: 41,
      processStartIdentity: "Mon Oct 12 10:00:00 2026",
      manifestSha256: sha("a"),
      actionSha256: sha("b"),
      uiSha256: sha("c"),
      windowId: 7,
    })).toBe(true);
    expect(acceptedRuntimeAck(ack, {
      pid: 41,
      processStartIdentity: "Mon Oct 12 10:00:00 2026",
      manifestSha256: sha("d"),
      actionSha256: sha("e"),
      uiSha256: sha("c"),
      windowId: 7,
    })).toBe(false);
    expect(acceptedRuntimeAck({ ...ack, renderers: [{ ...ack.renderers[0], state: "unconfirmed" }] }, {
      pid: 41,
      processStartIdentity: "Mon Oct 12 10:00:00 2026",
      manifestSha256: sha("a"),
      actionSha256: sha("b"),
      uiSha256: sha("c"),
      windowId: 7,
    })).toBe(false);
    expect(acceptedRuntimeAck({ ...ack, renderers: [{ ...ack.renderers[0], ui: sha("8") }] }, {
      pid: 41,
      processStartIdentity: "Mon Oct 12 10:00:00 2026",
      manifestSha256: sha("a"),
      actionSha256: sha("b"),
      uiSha256: sha("c"),
      windowId: 7,
    })).toBe(false);
    const expected = {
      pid: 41,
      processStartIdentity: "Mon Oct 12 10:00:00 2026",
      manifestSha256: sha("a"),
      actionSha256: sha("b"),
      uiSha256: sha("c"),
      windowId: 7,
    };
    const otherAcknowledged = { windowId: 8, generation: sha("a"), ui: sha("c"), state: "acknowledged" };
    expect(acceptedRuntimeAck({ ...ack, renderers: [...ack.renderers, otherAcknowledged] }, expected)).toBe(true);
    expect(acceptedRuntimeAck({ ...ack, renderers: [...ack.renderers, { ...otherAcknowledged, state: "pending" }] }, expected)).toBe(false);
    expect(acceptedRuntimeAck({ ...ack, renderers: [...ack.renderers, { ...otherAcknowledged, state: "unconfirmed" }] }, expected)).toBe(false);
    expect(acceptedRuntimeAck({ ...ack, renderers: [...ack.renderers, { ...otherAcknowledged, generation: sha("d") }] }, expected)).toBe(false);
    expect(acceptedRuntimeAck({ ...ack, renderers: [...ack.renderers, ack.renderers[0]] }, expected)).toBe(false);
  });

  test("checks the actual IPC action response without treating a request id as evidence", () => {
    expect(() => verifyActionResult(
      { requestId: "must-not-be-reported", ok: false, code: "HOT-ACCEPTANCE-B", reason: "hot-acceptance-b" },
      { ok: false, code: "HOT-ACCEPTANCE-B", reason: "hot-acceptance-b" },
    )).not.toThrow();
    expect(() => verifyActionResult(
      { requestId: "untrusted", ok: true, code: "OK" },
      { ok: false, code: "HOT-ACCEPTANCE-B", reason: "hot-acceptance-b" },
    )).toThrow("action result mismatch");
  });

  test("models B with the production open-result normalization", async () => {
    const actions = createMainActions({
      isIncognito: () => false,
      launchIncognito: async () => ({ ok: false, reason: "hot-acceptance-b" }),
    });
    expect(await actions.handle("open", {}, undefined)).toEqual({
      ok: false,
      code: "HOT-ACCEPTANCE-B",
      reason: "hot-acceptance-b",
    });
  });

  test("checks each frozen CLI's reported version", () => {
    expect(() => verifyCliVersion("Incodex version 1.4.0\n", "1.4.0", "A")).not.toThrow();
    expect(() => verifyCliVersion("Incodex version 1.4.1\n", "1.4.0", "A")).toThrow("version does not match");
  });
});
