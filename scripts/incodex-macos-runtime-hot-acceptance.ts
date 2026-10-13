#!/usr/bin/env bun
/**
 * [INPUT]: 依赖冻结的 A/B CLI 证据、Runtime 合同和 macOS 宿主适配器。
 * [OUTPUT]: 生成 A→B→A 实机验收报告；仅正常关闭本脚本创建的窗口。
 * [POS]: 显式 Runtime 热切换验收入口，不属于 install、update 或日常启动链。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { dirname, join } from "node:path";
import { homedir } from "node:os";
import { randomUUID } from "node:crypto";
import { chmodSync, existsSync, lstatSync, mkdirSync, rmSync } from "node:fs";
import { spawn } from "node:child_process";
import {
  ACK_TIMEOUT_MS, ACTION_ARTIFACT, POLL_MS, PRODUCT_BINARY, STARTUP_TIMEOUT_MS,
  acceptedRuntimeAck, assertPairEvidence, isHash, parseArguments, readCandidate,
  readCurrentPointer, requireCurrentCandidate, requirePrivateTmpOutput, requireExecutable,
  runCli, usage, verifyActionResult,
} from "./incodex-macos-runtime-hot-acceptance-contract.ts";
import type { AckExpectation, ActionExpectation, CandidateEvidence, Options, Report, RuntimeAck } from "./incodex-macos-runtime-hot-acceptance-contract.ts";
import {
  addEvent, acceptedAckCount, checkAppAndRuntimePreflight, closePrivateWindow,
  copyAllowlistedCodexFiles, currentSessionIds, delay, invokeProductionOpen,
  foreignOfficialHostPids, isolatedHostEnvironment, officialHostPids, pageTargets, processDefinitelyExited, processIsAlive, processStartIdentity, readAckEvents,
  reservePort, runAxCompile, sentinelCheck, sourceCodexHome, waitForAck, waitForCurrent,
  waitForMainPage, waitForNewSession, waitForPort, waitForSessionBurn, writeSafeReport,
} from "./incodex-macos-runtime-hot-acceptance-host.ts";
import type { CdpClient, CdpTarget } from "./incodex-macos-runtime-hot-acceptance-host.ts";

function createReport(): Report {
  return {
    schemaVersion: 1,
    startedAt: new Date().toISOString(),
    candidateA: "A",
    candidateB: "B",
    host: { pid: null, processStartIdentity: null, windowId: null },
    proof: {},
    events: [],
    result: "running",
    failure: null,
    restoreA: "not-needed",
    hostClose: "not-started",
  };
}

function recordAck(report: Report, label: string, row: RuntimeAck): void {
  addEvent(report, {
    phase: label,
    runtimeGeneration: row.main?.generation ?? null,
    actionArtifact: row.main?.artifact ?? null,
    processId: row.pid ?? null,
    processStartIdentity: row.processStartIdentity ?? null,
    windowId: row.renderers?.[0]?.windowId ?? null,
    rendererState: row.renderers?.[0]?.state ?? null,
  });
}

async function runAcceptance(options: Options): Promise<Report> {
  if (!options.aCli || !options.aEvidence || !options.bCli || !options.bEvidence || !options.output) {
    throw new Error("--run requires the two CLI/evidence pairs and --out");
  }
  const a = readCandidate(options.aEvidence, "A");
  const b = readCandidate(options.bEvidence, "B");
  const pairProof = assertPairEvidence({ a, b });
  const aCli = requireExecutable(options.aCli, a, "A");
  const bCli = requireExecutable(options.bCli, b, "B");
  requirePrivateTmpOutput(options.output);
  checkAppAndRuntimePreflight(aCli, bCli, a, b);
  const source = sourceCodexHome(options.sourceCodexHome);

  mkdirSync(options.output, { mode: 0o700 });
  chmodSync(options.output, 0o700);
  const reportPath = join(options.output, "report.json");
  const report = createReport();
  let runtimeTouched = false;
  let host: ReturnType<typeof spawn> | null = null;
  let cdp: CdpClient | null = null;
  let target: CdpTarget | null = null;
  let hostStart = "";
  let isolatedCodexHome = "";
  let isolatedUserData = "";
  let axHelper = "";
  let openChildren: Array<{ id: string; owner: { pid: number; processStartIdentity: string } }> = [];
  let pass = false;

  const requireOnlyOwnedHosts = (restoring = false) => {
    const ownedStarts = new Map<number, string>();
    if (host?.pid && hostStart) ownedStarts.set(host.pid, hostStart);
    for (const child of openChildren) ownedStarts.set(child.owner.pid, child.owner.processStartIdentity);
    const foreign = foreignOfficialHostPids(officialHostPids(), ownedStarts);
    if (foreign.length && !restoring) throw new Error("another official host appeared during acceptance; it was not touched");
    if (foreign.length && restoring) report.proof.externalHostPresentDuringARecovery = true;
  };
  const publish = async (cli: string, evidence: CandidateEvidence, label: string, restoring = false) => {
    requireOnlyOwnedHosts(restoring);
    runtimeTouched = true;
    runCli(cli, ["runtime"], `${label} Runtime publication`);
    requireOnlyOwnedHosts(restoring);
    await waitForCurrent(evidence);
    requireOnlyOwnedHosts(restoring);
    addEvent(report, { phase: `${label}-published`, runtimeGeneration: evidence.runtimeManifestSha256 });
  };
  const requireStableHostAndSentinel = async (port: number, expectedTarget: CdpTarget, expectedPid: number, expectedStart: string, sentinel: string) => {
    const currentStart = processStartIdentity(expectedPid);
    if (currentStart !== expectedStart) throw new Error("isolated host PID/start identity changed");
    const targets = await pageTargets(port);
    if (targets.length !== 1 || targets[0]!.id !== expectedTarget.id || targets[0]!.webSocketDebuggerUrl !== expectedTarget.webSocketDebuggerUrl) {
      throw new Error("isolated main renderer target changed during hot activation");
    }
    if (!cdp || !(await sentinelCheck(cdp, sentinel, false))) throw new Error("isolated renderer sentinel changed during hot activation");
  };
  const waitGeneration = async (expected: AckExpectation, label: string, previousCount: number) => {
    const row = await waitForAck(expected, previousCount);
    recordAck(report, label, row);
    report.proof[`${label}ActiveAck`] = true;
  };
  const openAndClosePrivateWindow = async (expectation: ActionExpectation) => {
    if (!cdp) throw new Error("isolated renderer is unavailable");
    requireOnlyOwnedHosts();
    const before = currentSessionIds();
    await invokeProductionOpen(cdp, expectation);
    if (expectation.ok) {
      const child = await waitForNewSession(before);
      openChildren.push(child);
      if (!processIsAlive(child.owner.pid, child.owner.processStartIdentity)) throw new Error("new private process identity is not live");
      const axWindow = closePrivateWindow(axHelper, child.owner);
      await waitForSessionBurn(child.id, child.owner);
      openChildren = openChildren.filter((entry) => entry.id !== child.id);
      addEvent(report, { phase: "A-opened-and-normally-closed", nativeWindowId: axWindow });
    } else {
      await delay(750);
      const after = currentSessionIds();
      if (after.size !== before.size || [...after].some((id) => !before.has(id))) {
        throw new Error("B denial unexpectedly created an Incodex private session");
      }
      addEvent(report, { phase: "B-denied-without-session", resultCode: expectation.code });
    }
  };

  try {
    isolatedCodexHome = join(options.output, "isolated-codex-home");
    isolatedUserData = join(options.output, "isolated-chromium-user-data");
    mkdirSync(isolatedUserData, { recursive: false, mode: 0o700 });
    chmodSync(isolatedUserData, 0o700);
    const copied = copyAllowlistedCodexFiles(source, isolatedCodexHome);
    report.proof.allowlistedCodexCopy = copied.every((name) => ["auth.json", "config.toml"].includes(name));
    report.proof.realHomePreserved = homedir() === process.env.HOME;
    report.proof.dailyHostAbsentAtStart = true;
    report.proof.noIncognitoEnvironment = true;
    addEvent(report, { phase: "isolated-inputs-ready", copiedAllowlistCount: copied.length });

    axHelper = runAxCompile(options.output);
    await publish(aCli, a, "A-initial");
    if (officialHostPids().length) throw new Error("an official host appeared after A publication; no host was closed");
    const port = await reservePort();
    const env = isolatedHostEnvironment(isolatedCodexHome, isolatedUserData);
    if (env.INCODEX_INCOGNITO || env.INCODEX_SESSION_ROOT || env.INCODEX_SESSION_ID) throw new Error("incognito launch environment was not cleared");
    host = spawn(PRODUCT_BINARY, [
      `--user-data-dir=${isolatedUserData}`,
      "--remote-debugging-address=127.0.0.1",
      `--remote-debugging-port=${port}`,
      `--remote-allow-origins=http://127.0.0.1:${port}`,
    ], { detached: false, stdio: "ignore", env });
    if (!host.pid) throw new Error("isolated host did not provide a PID");
    host.unref();
    const pid = host.pid;
    const identityDeadline = Date.now() + STARTUP_TIMEOUT_MS;
    while (!hostStart && Date.now() < identityDeadline) {
      hostStart = processStartIdentity(pid) ?? "";
      if (!hostStart) await delay(POLL_MS);
    }
    if (!hostStart) throw new Error("isolated host process-start identity is unavailable");
    report.host.pid = pid;
    report.host.processStartIdentity = hostStart;
    await waitForPort(pid, port);
    const selected = await waitForMainPage(port);
    target = selected.target;
    cdp = selected.cdp;
    const sentinel = randomUUID();
    if (!(await sentinelCheck(cdp, sentinel, true))) throw new Error("normal-host sentinel could not be installed");
    const initialAck: AckExpectation = {
      pid,
      processStartIdentity: hostStart,
      manifestSha256: a.runtimeManifestSha256,
      actionSha256: a.runtimeFiles[ACTION_ARTIFACT]!,
      uiSha256: a.runtimeFiles["incodex-inject.js"]!,
      windowId: 0,
    };
    // First ACK identifies the current top-level window. The window id is not guessed from CDP.
    const deadline = Date.now() + ACK_TIMEOUT_MS;
    let ackA: RuntimeAck | null = null;
    while (Date.now() < deadline) {
      const rows = readAckEvents().reverse();
      ackA = rows.find((row) => row.phase === "active" && row.pid === pid && row.processStartIdentity === hostStart &&
        row.published?.generation === a.runtimeManifestSha256 && row.main?.generation === a.runtimeManifestSha256 &&
        row.main?.artifact === a.runtimeFiles[ACTION_ARTIFACT] && row.restartRequired === false &&
        row.installRequired === false && row.rollbackFailed === false && row.renderers?.length === 1 &&
        row.renderers[0]?.state === "acknowledged" && row.renderers[0]?.generation === a.runtimeManifestSha256 &&
        isHash(row.renderers[0]?.ui)) ?? null;
      if (ackA) break;
      await delay(POLL_MS);
    }
    if (!ackA || !Number.isSafeInteger(ackA.renderers?.[0]?.windowId) || ackA.renderers![0]!.windowId! <= 0) {
      throw new Error("initial A active ACK did not identify one acknowledged renderer window");
    }
    const windowId = ackA.renderers![0]!.windowId!;
    report.host.windowId = windowId;
    const initialExpectation = { ...initialAck, windowId };
    if (!acceptedRuntimeAck(ackA, initialExpectation)) throw new Error("initial A generation ACK failed exact identity checks");
    recordAck(report, "A-initial", ackA);
    report.proof.AInitialActiveAck = true;
    if (!(await sentinelCheck(cdp, sentinel, false))) throw new Error("normal-host sentinel changed before A action");
    await openAndClosePrivateWindow({ ok: true, code: "OK" });
    report.proof.AOpenActuallyCreatedAndBurnedPrivateSession = true;
    await requireStableHostAndSentinel(port, target, pid, hostStart, sentinel);

    const expectedB: AckExpectation = {
      pid, processStartIdentity: hostStart, manifestSha256: b.runtimeManifestSha256,
      actionSha256: b.runtimeFiles[ACTION_ARTIFACT]!, uiSha256: b.runtimeFiles["incodex-inject.js"]!, windowId,
    };
    const previousBCount = acceptedAckCount(expectedB);
    await publish(bCli, b, "B");
    await waitGeneration(expectedB, "B", previousBCount);
    await requireStableHostAndSentinel(port, target, pid, hostStart, sentinel);
    requireOnlyOwnedHosts();
    const beforeB = currentSessionIds();
    await invokeProductionOpen(cdp, { ok: false, code: pairProof.code, reason: pairProof.reason });
    await delay(750);
    const afterB = currentSessionIds();
    if (afterB.size !== beforeB.size || [...afterB].some((id) => !beforeB.has(id))) {
      throw new Error("B action denial created a private session");
    }
    report.proof.BActionDeniedThroughProductionIpc = true;
    report.proof.BDenialCreatedNoPrivateSession = true;
    addEvent(report, { phase: "B-action-denied", resultCode: pairProof.code });
    await requireStableHostAndSentinel(port, target, pid, hostStart, sentinel);

    const restoredA: AckExpectation = {
      pid, processStartIdentity: hostStart, manifestSha256: a.runtimeManifestSha256,
      actionSha256: a.runtimeFiles[ACTION_ARTIFACT]!, uiSha256: a.runtimeFiles["incodex-inject.js"]!, windowId,
    };
    const previousACount = acceptedAckCount(restoredA);
    await publish(aCli, a, "A-restore");
    await waitGeneration(restoredA, "A-restored", previousACount);
    await requireStableHostAndSentinel(port, target, pid, hostStart, sentinel);
    await openAndClosePrivateWindow({ ok: true, code: "OK" });
    report.proof.AAfterBActuallyCreatedAndBurnedPrivateSession = true;
    await requireStableHostAndSentinel(port, target, pid, hostStart, sentinel);
    pass = true;
    report.result = "passed";
    report.proof.sameProcessStartWindowAndSentinelAcrossABA = true;
  } catch (error) {
    report.result = "failed";
    report.failure = error instanceof Error ? error.message : "acceptance failed";
  } finally {
    for (const child of [...openChildren]) {
      try {
        if (processIsAlive(child.owner.pid, child.owner.processStartIdentity)) closePrivateWindow(axHelper, child.owner);
        await waitForSessionBurn(child.id, child.owner);
        openChildren = openChildren.filter((entry) => entry.id !== child.id);
      } catch {
        addEvent(report, { phase: "owned-private-window-remains-open", safeCloseCompleted: false });
      }
    }
    if (runtimeTouched) {
      try {
        const current = readCurrentPointer();
        if (current.pointer.manifestSha256 !== a.runtimeManifestSha256) await publish(aCli, a, "A-final-restore", true);
        requireCurrentCandidate(readCurrentPointer(), a, "A restoration");
        report.restoreA = "restored";
      } catch {
        report.restoreA = "failed";
        report.result = "failed";
        report.failure ??= "A Runtime restoration failed; preserve evidence and do not remove the isolated files";
      }
    }
    if (host?.pid && hostStart) {
      try {
        if (processIsAlive(host.pid, hostStart)) {
          const axWindowId = closePrivateWindow(axHelper, { pid: host.pid, processStartIdentity: hostStart });
          report.hostClose = "window-closed";
          addEvent(report, { phase: "owned-normal-window-closed", nativeWindowId: axWindowId });
        } else report.hostClose = "window-closed";
      } catch {
        report.hostClose = "window-close-failed";
        report.result = "failed";
        report.failure ??= "owned normal-host window did not accept its exact native AX close; host process was not terminated";
      }
    } else if (host?.pid) {
      report.hostClose = "window-close-failed";
      report.result = "failed";
      report.failure ??= "isolated host start identity is unavailable; no close or deletion was attempted";
    }
    cdp?.close();
    if (host?.pid && hostStart) {
      const exitDeadline = Date.now() + 5_000;
      while (processIsAlive(host.pid, hostStart) && Date.now() < exitDeadline) await delay(POLL_MS);
    }
    // No kill/terminate path exists. If the process remains alive, retain its private homes for safety.
    const hostDefinitelyExited = !host?.pid || (Boolean(hostStart) && processDefinitelyExited(host.pid!, hostStart));
    if (!hostDefinitelyExited) {
      report.proof.isolatedHostStillRunningAfterWindowClose = true;
      addEvent(report, { phase: "host-process-left-running", reason: hostStart ? "normal-close-only" : "identity-unavailable" });
    } else {
      try {
        for (const directory of [isolatedCodexHome, isolatedUserData]) {
          if (!directory || !existsSync(directory)) continue;
          const stat = lstatSync(directory);
          if (stat.isSymbolicLink() || !stat.isDirectory() || dirname(directory) !== options.output ||
              (typeof process.getuid === "function" && stat.uid !== process.getuid())) {
            report.result = "failed";
            report.failure ??= "isolated temporary directory identity changed; it was not removed";
          } else {
            rmSync(directory, { recursive: true, force: false });
          }
        }
        report.proof.isolatedCredentialCopyRemovedAfterHostExit = true;
      } catch {
        report.proof.isolatedCredentialCopyRemovedAfterHostExit = false;
        report.result = "failed";
        report.failure ??= "isolated temporary data could not be removed after host exit";
      }
    }
    report.proof.runtimeRestoredToA = report.restoreA === "restored";
    if (!pass && report.result === "running") report.result = "failed";
    try { writeSafeReport(reportPath, report); }
    catch {
      report.result = "failed";
      report.failure ??= "private acceptance report could not be written";
    }
  }
  return report;
}

function writeFailureReport(output: string, aId: string, bId: string, error: unknown): void {
  try {
    const stat = lstatSync(output);
    if (stat.isSymbolicLink() || !stat.isDirectory() || (stat.mode & 0o077) !== 0 ||
        (typeof process.getuid === "function" && stat.uid !== process.getuid()) || dirname(output) !== "/private/tmp") return;
    writeSafeReport(join(output, "report.json"), {
      schemaVersion: 1, startedAt: new Date().toISOString(), candidateA: aId, candidateB: bId,
      host: { pid: null, processStartIdentity: null, windowId: null }, proof: {}, events: [],
      result: "failed", failure: error instanceof Error ? error.message : "preflight failed",
      restoreA: "not-needed", hostClose: "not-started",
    });
  } catch { /* no private evidence directory is available */ }
}

function selfTest(): void {
  const proof = {
    schemaVersion: 1, candidateId: "A", cliVersion: "1.0.0", cliSha256: "a".repeat(64), runtimeVersion: "1.0.0",
    runtimeManifestSha256: "b".repeat(64), runtimeSourceManifestSha256: "c".repeat(64),
    runtimeFiles: { "incodex-inject.js": "d".repeat(64), [ACTION_ARTIFACT]: "e".repeat(64), "incodex-main.cjs": "f".repeat(64), "incodex-preload.cjs": "9".repeat(64) },
  };
  const b = { ...proof, candidateId: "B", cliSha256: "1".repeat(64), runtimeManifestSha256: "2".repeat(64),
    runtimeSourceManifestSha256: "3".repeat(64), runtimeFiles: { ...proof.runtimeFiles, [ACTION_ARTIFACT]: "4".repeat(64) },
    hotAcceptance: { kind: "bound-open-denial", action: "open", artifact: ACTION_ARTIFACT, code: "HOT-ACCEPTANCE-B", reason: "hot-acceptance-b", artifactSha256: "4".repeat(64) } };
  assertPairEvidence({ a: proof, b });
  verifyActionResult({ requestId: "not-retained", ok: false, code: "HOT-ACCEPTANCE-B", reason: "hot-acceptance-b" },
    { ok: false, code: "HOT-ACCEPTANCE-B", reason: "hot-acceptance-b" });
  if (parseArguments(["--self-test"]).mode !== "self-test") throw new Error("self-test parser failed");
  console.log("macOS Runtime hot acceptance pure self-test passed; no App, CLI, or Runtime was touched.");
}

async function main(): Promise<void> {
  let options: Options;
  try { options = parseArguments(process.argv.slice(2)); }
  catch (error) {
    const message = error instanceof Error ? error.message : "invalid arguments";
    if (message === usage()) console.log(message);
    else console.error(`ERROR: ${message}`);
    process.exitCode = message === usage() ? 0 : 2;
    return;
  }
  if (options.mode === "self-test") { selfTest(); return; }
  try {
    const report = await runAcceptance(options);
    console.log(`Runtime A→B→A acceptance ${report.result}.`);
    console.log(`Private evidence: ${join(options.output!, "report.json")}`);
    if (report.result !== "passed") process.exitCode = 1;
  } catch (error) {
    writeFailureReport(options.output!, "A", "B", error);
    console.error(`ERROR: ${error instanceof Error ? error.message : "preflight failed"}`);
    process.exitCode = 1;
  }
}

if (import.meta.main) void main();
