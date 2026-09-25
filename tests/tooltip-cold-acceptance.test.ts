import { describe, expect, test } from "bun:test";
import {
  assertLoopbackListenerOwnership,
  assertRuntimeCandidate,
  coldLatencyFailure,
  findDebuggerChild,
  findDebuggerChildIfStarted,
  parseArguments,
  parseDryRunTargets,
  parseListenerRows,
  parseProcessTable,
  parseRendererPrepareWarning,
  tooltipEventProbeExpression,
  tooltipStateExpression,
  validateWebSocketUrl,
} from "../scripts/incodex-tooltip-cold-acceptance.ts";

describe("installed tooltip acceptance safety contracts", () => {
  test("run mode requires an explicit CLI, exact parent PID, and new output path", () => {
    expect(parseArguments([
      "--run", "--cli", "/tmp/incodex", "--parent-pid", "60336", "--out", "/private/tmp/tooltip-evidence",
    ])).toEqual({
      mode: "run", cli: "/tmp/incodex", parentPid: 60336,
      output: "/private/tmp/tooltip-evidence", toleranceMs: 300,
    });
    expect(() => parseArguments(["--run", "--cli", "/tmp/incodex", "--parent-pid", "0", "--out", "/tmp/evidence"]))
      .toThrow(/positive PID/u);
    expect(() => parseArguments(["--run", "--cli", "incodex", "--parent-pid", "60336", "--out", "/tmp/evidence"]))
      .toThrow(/absolute executable/u);
    expect(() => parseArguments(["--run", "--cli", "/tmp/incodex", "--parent-pid", "60336"]))
      .toThrow(/new absolute output/u);
    expect(() => parseArguments(["--self-test", "--out", "/tmp/evidence"]))
      .toThrow(/does not accept run options/u);
  });

  test("installed-candidate gate requires the exact official package and current CLI-matched Runtime", () => {
    const valid = {
      target: "/Applications/ChatGPT.app",
      exists: true,
      patched: true,
      bundleId: "com.openai.codex",
      asarLoaderOnly: true,
      asarFileHash: "a".repeat(64),
      plistFileHash: "b".repeat(64),
      externalRuntime: { ok: true, matchesBundled: true, state: "current" },
    };
    expect(() => assertRuntimeCandidate(valid, "/Applications/ChatGPT.app")).not.toThrow();
    for (const changed of [
      { ...valid, patched: false },
      { ...valid, bundleId: "com.example.other" },
      { ...valid, asarLoaderOnly: false },
      { ...valid, externalRuntime: { ok: true, matchesBundled: false, state: "stale" } },
    ]) {
      expect(() => assertRuntimeCandidate(changed, "/Applications/ChatGPT.app")).toThrow(/current Runtime/u);
    }
  });

  test("dry-run and process parsing bind only the CLI-spawned exact app with a debugger port", () => {
    const targets = parseDryRunTargets([
      "  App          /Applications/ChatGPT.app",
      "  Binary       /Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
      "  ! Dry run. No window opened.",
    ].join("\n"));
    expect(targets.binary).toBe("/Applications/ChatGPT.app/Contents/MacOS/ChatGPT");
    expect(() => parseDryRunTargets("App /Applications/ChatGPT.app\nBinary /tmp/other")).toThrow(/dry-run/u);

    const rows = parseProcessTable([
      "  234 123 Sat Sep 26 04:18:00 2026 /Applications/ChatGPT.app/Contents/MacOS/ChatGPT --remote-debugging-port=56789",
      "  235 123 Sat Sep 26 04:18:01 2026 /Applications/ChatGPT.app/Contents/Frameworks/ChatGPT Helper.app/Contents/MacOS/ChatGPT Helper",
    ].join("\n"));
    expect(findDebuggerChildIfStarted(rows, 456, targets.binary)).toBeNull();
    expect(findDebuggerChild(rows, 123, targets.binary)).toMatchObject({ pid: 234, ppid: 123, port: 56789 });
    expect(() => findDebuggerChild(rows, 456, targets.binary)).toThrow(/found 0/u);
    expect(() => findDebuggerChild([...rows, { ...rows[0]!, pid: 236 }], 123, targets.binary)).toThrow(/found 2/u);
  });

  test("CDP listeners must belong only to the isolated PID on a loopback address", () => {
    const listeners = parseListenerRows([
      "COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME",
      "ChatGPT 234 d 60u IPv4 0xabc 0t0 TCP 127.0.0.1:56789 (LISTEN)",
      "ChatGPT 234 d 61u IPv6 0xdef 0t0 TCP [::1]:56789 (LISTEN)",
    ].join("\n"));
    expect(() => assertLoopbackListenerOwnership(listeners, 234, 56789)).not.toThrow();
    expect(() => assertLoopbackListenerOwnership([{ pid: 999, endpoint: "127.0.0.1:56789" }], 234, 56789))
      .toThrow(/not owned exclusively/u);
    expect(() => assertLoopbackListenerOwnership([{ pid: 234, endpoint: "*:56789" }], 234, 56789))
      .toThrow(/loopback/u);
    expect(() => assertLoopbackListenerOwnership([
      { pid: 234, endpoint: "127.0.0.1:56789" }, { pid: 999, endpoint: "127.0.0.1:56789" },
    ], 234, 56789)).toThrow(/not owned exclusively/u);
    expect(() => validateWebSocketUrl("ws://127.0.0.1:56789/devtools/page/x", 56789).hostname).not.toThrow();
    expect(() => validateWebSocketUrl("ws://192.0.2.2:56789/devtools/page/x", 56789)).toThrow(/loopback/u);
    expect(() => validateWebSocketUrl("ws://127.0.0.1:56790/devtools/page/x", 56789)).toThrow(/loopback/u);
  });

  test("latency comparison catches an extra cold delay and allows documented measurement overhead", () => {
    expect(coldLatencyFailure(802, 724, 100)).toBeNull();
    expect(coldLatencyFailure(1_402, 724, 300)).toMatch(/slower than official Search/u);
    expect(coldLatencyFailure(-1, 724, 300)).toMatch(/non-negative/u);
  });

  test("cold observation records provider timing and renderer state without waiting for readiness", () => {
    const state = tooltipStateExpression();
    expect(state).toContain("rendererReady");
    expect(state).toContain("tooltipRootCount");
    expect(state).toContain("searchTooltipSampleAvailable");
    expect(state).toContain("getOpenDelay('default',700)");
    expect(state).toContain("isHoverOpenBlocked('incodex-privacy-toggle')");
    expect(state).toContain("skipDelayDuration");
    expect(state).toContain("tooltipCandidates");
    expect(state).toContain("indexScriptSrc");
    expect(state).not.toContain("innerText");
    expect(state).not.toContain("textContent");
    expect(() => new Function(`return ${state}`)).not.toThrow();

    const events = tooltipEventProbeExpression();
    expect(events).toContain("pointerenter");
    expect(events).toContain("e.isTrusted");
    expect(events).toContain("e.key==='Escape'");
    expect(events).not.toContain("innerText");
    expect(() => new Function(`return ${events}`)).not.toThrow();
  });

  test("renderer warning output is reduced to a known error code and internal asset paths", () => {
    expect(parseRendererPrepareWarning("Error: Unsupported official Tooltip exports at app://-/assets/tooltip-abc123.js"))
      .toEqual({ errorCode: "UNSUPPORTED_OFFICIAL_TOOLTIP_EXPORTS", modulePaths: ["app://-/assets/tooltip-abc123.js"] });
    const result = parseRendererPrepareWarning("TypeError: private words must not be kept https://example.invalid app://-/assets/react-xyz.js");
    expect(result).toEqual({ errorCode: "TYPEERROR", modulePaths: ["app://-/assets/react-xyz.js"] });
    expect(JSON.stringify(result)).not.toContain("private words");
    expect(JSON.stringify(result)).not.toContain("example.invalid");
  });
});
