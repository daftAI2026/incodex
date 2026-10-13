import { describe, expect, test } from "bun:test";
import {
  acceptedRuntimeAck,
  assertPairEvidence,
  parseArguments,
  verifyActionResult,
  verifyCliVersion,
} from "./incodex-macos-runtime-hot-acceptance-contract.ts";

const sha = (n: string) => n.repeat(64).slice(0, 64);

function candidate(overrides: Record<string, unknown> = {}) {
  return {
    schemaVersion: 1,
    candidateId: "candidate-a",
    cliVersion: "1.4.0",
    cliSha256: sha("a"),
    runtimeVersion: "1.4.0",
    runtimeManifestSha256: sha("b"),
    runtimeSourceManifestSha256: sha("c"),
    runtimeFiles: {
      "incodex-inject.js": sha("d"),
      "incodex-main-actions.cjs": sha("e"),
      "incodex-main.cjs": sha("f"),
      "incodex-preload.cjs": sha("9"),
    },
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
      runtimeFiles: {
        "incodex-inject.js": sha("d"),
        "incodex-main-actions.cjs": sha("4"),
        "incodex-main.cjs": sha("f"),
        "incodex-preload.cjs": sha("9"),
      },
      hotAcceptance: {
        kind: "bound-open-denial",
        action: "open",
        artifact: "incodex-main-actions.cjs",
        code: "HOT_ACCEPTANCE_B",
        reason: "hot-acceptance-b",
        artifactSha256: sha("4"),
      },
    }),
  };
}

describe("macOS Runtime hot acceptance guards", () => {
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
      code: "HOT_ACCEPTANCE_B",
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
  });

  test("checks the actual IPC action response without treating a request id as evidence", () => {
    expect(() => verifyActionResult(
      { requestId: "must-not-be-reported", ok: false, code: "HOT_ACCEPTANCE_B", reason: "hot-acceptance-b" },
      { ok: false, code: "HOT_ACCEPTANCE_B", reason: "hot-acceptance-b" },
    )).not.toThrow();
    expect(() => verifyActionResult(
      { requestId: "untrusted", ok: true, code: "OK" },
      { ok: false, code: "HOT_ACCEPTANCE_B", reason: "hot-acceptance-b" },
    )).toThrow("action result mismatch");
  });

  test("checks each frozen CLI's reported version", () => {
    expect(() => verifyCliVersion("Incodex version 1.4.0\n", "1.4.0", "A")).not.toThrow();
    expect(() => verifyCliVersion("Incodex version 1.4.1\n", "1.4.0", "A")).toThrow("version does not match");
  });
});
