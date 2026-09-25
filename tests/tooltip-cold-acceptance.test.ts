import { describe, expect, test } from "bun:test";
import {
  assertLoopbackListenerOwnership,
  assertRuntimeCandidate,
  coldLatencyFailure,
  liveCapabilityProbeExpression,
  findDebuggerChild,
  findDebuggerChildIfStarted,
  maySendEscapeToDismissAcceptanceTooltip,
  parseArguments,
  parseDryRunTargets,
  parseListenerRows,
  parseProcessTable,
  parseRendererPrepareWarning,
  verifiedDescendantPids,
  waitForStableHitTest,
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
    const childTree = verifiedDescendantPids([
      { pid: 234, ppid: 123, started: "child", command: "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT --remote-debugging-port=56789" },
      { pid: 626, ppid: 234, started: "service", command: "/Applications/ChatGPT.app/Contents/Frameworks/SkyComputerUseService" },
      { pid: 627, ppid: 626, started: "nested", command: "/Applications/ChatGPT.app/Contents/Frameworks/helper" },
    ], 234);
    expect([...childTree].sort((a, b) => a - b)).toEqual([234, 626, 627]);
    expect(() => assertLoopbackListenerOwnership([
      { pid: 234, endpoint: "127.0.0.1:56789" }, { pid: 626, endpoint: "127.0.0.1:56789" },
    ], 234, 56789, childTree)).not.toThrow();
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

  test("Escape is sent only to dismiss this run's visible tooltip in the focused document", () => {
    expect(maySendEscapeToDismissAcceptanceTooltip(true, true, true)).toBe(true);
    expect(maySendEscapeToDismissAcceptanceTooltip(false, true, true)).toBe(false);
    expect(maySendEscapeToDismissAcceptanceTooltip(true, false, true)).toBe(false);
    expect(maySendEscapeToDismissAcceptanceTooltip(true, true, false)).toBe(false);
  });

  test("input readiness waits for stable hit targets and reports obstruction without guessing", async () => {
    const samples = [
      { documentFocused: true, hatHitTest: false, searchHitTest: false, marker: "blocked" },
      { documentFocused: true, hatHitTest: false, searchHitTest: false, marker: "blocked" },
      { documentFocused: true, hatHitTest: true, searchHitTest: true, marker: "ready" },
      { documentFocused: true, hatHitTest: true, searchHitTest: true, marker: "ready" },
      { documentFocused: true, hatHitTest: true, searchHitTest: true, marker: "ready" },
    ];
    const ready = await waitForStableHitTest(
      async () => samples.shift() ?? { documentFocused: true, hatHitTest: true, searchHitTest: true, marker: "ready" },
      { exited: false, exitCode: null },
      1_000,
      3,
      1,
    );
    expect(ready.stable).toBe(true);
    expect(ready.samples).toBe(5);
    expect(ready.firstBlocked?.marker).toBe("blocked");
    expect(ready.state.marker).toBe("ready");

    const blocked = await waitForStableHitTest(
      async () => ({ documentFocused: true, hatHitTest: false, searchHitTest: false, marker: "covered" }),
      { exited: false, exitCode: null },
      3,
      3,
      1,
    );
    expect(blocked.stable).toBe(false);
    expect(blocked.firstBlocked?.marker).toBe("covered");
    expect(blocked.state.marker).toBe("covered");
  });

  test("cold observation records provider timing and renderer state without waiting for readiness", () => {
    const state = tooltipStateExpression();
    expect(state).toContain("rendererReady");
    expect(state).toContain("tooltipRootCount");
    expect(state).toContain("documentFocused");
    expect(state).toContain("hatHitTest");
    expect(state).toContain("searchHitTest");
    expect(state).toContain("hatHitTarget");
    expect(state).toContain("searchHitTarget");
    expect(state).toContain("ariaModal");
    expect(state).toContain("data-testid");
    expect(state).toContain("idHash");
    expect(state).toContain("classHash");
    expect(state).not.toContain("outerHTML");
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
    expect(events).toContain("sequence:++window.__incodexTooltipAcceptanceSequence");
    expect(events).not.toContain("innerText");
    expect(() => new Function(`return ${events}`)).not.toThrow();
  });

  test("live module capability probe is bounded, identity-based, and content-free", () => {
    const probe = liveCapabilityProbeExpression();
    expect(probe).toContain("directStaticModuleCount");
    expect(probe).toContain("reactFacadeMatchCount");
    expect(probe).toContain("tooltipNamespaceIdentityMatchCount");
    expect(probe).toContain("candidate.type===value");
    expect(probe).toContain("tooltipDistinctContextCount");
    expect(probe).toContain("tooltipMatchingProviderFiberCount");
    expect(probe).toContain("await import(url)");
    expect(probe).toContain("PROBE_TIMEOUT");
    expect(probe).not.toContain("Function.prototype.toString");
    expect(probe).not.toContain("innerText");
    expect(probe).not.toContain("textContent");
    expect(probe).not.toContain("outerHTML");
    expect(() => new Function(`return ${probe}`)).not.toThrow();
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
