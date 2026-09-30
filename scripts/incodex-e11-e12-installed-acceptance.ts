#!/usr/bin/env bun
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { strict as assert } from "node:assert";
import {
  closeSync,
  constants as fsConstants,
  fstatSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { basename, isAbsolute, join, resolve } from "node:path";
import { COPY } from "../src/runtime/incognito-copy.ts";
import { SEARCH_LABELS } from "../src/runtime/compatibility/search-labels.ts";

const root = resolve(import.meta.dir, "..");
const cli = join(root, "target/release/incodex");
const helperSource = join(import.meta.dir, "incodex-e11-e12-installed-ax.swift");
const appPath = "/Applications/ChatGPT.app";
const executablePath = `${appPath}/Contents/MacOS/ChatGPT`;
const bundleIdentifier = "com.openai.codex";
const userRoot = join(homedir(), ".incodex");
const targetId = createHash("sha256").update(executablePath).digest("hex").slice(0, 12);
const targetSessions = join(userRoot, "sessions", targetId);
const sessionName = /^s-[A-Za-z0-9]+$/u;
const processStartIdentityPattern = /^(?:Sun|Mon|Tue|Wed|Thu|Fri|Sat)\s+(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+\d{1,2}\s+\d{2}:\d{2}:\d{2}\s+\d{4}$/u;
const labelsByKind = {
  open: [...new Set(Object.values(COPY).map((copy) => copy.open))],
  exit: [...new Set(Object.values(COPY).map((copy) => copy.exit))],
  search: [...SEARCH_LABELS],
};

type Mode = "preflight" | "run" | "self-test";
type Options = { mode: Mode; parentPid?: number; output?: string };
type Bounds = { x: number; y: number; width: number; height: number };
type BoundsComparison = { source: Bounds; child: Bounds; delta: { x: number; y: number }; sameSize: boolean; chromeTile22: boolean };
type OwnerBinding = { sessionId: string; pid: number; processStartIdentity: string; parentChainPids?: number[] };
type AxWindow = {
  windowId: number;
  bounds: Bounds;
  layer: number;
  alpha: number;
  zIndex: number;
  isMain: boolean;
  isMinimized: boolean;
  actions: string[];
};
type AxToggle = {
  role: string;
  label: string;
  kind: "open" | "exit";
  enabled: boolean;
  value: unknown;
  bounds: Bounds;
  windowId: number;
  windowBounds: Bounds;
  actions: string[];
};
type AxSearchButton = { role: string; label: string; bounds: Bounds; windowId: number };
type HelperReply = {
  ok: boolean;
  error?: string;
  pid?: number;
  matchingPids?: number[];
  frontmostPid?: number | null;
  frontmostBundleId?: string | null;
  securityAgentActive?: boolean;
  systemSettingsActive?: boolean;
  securityAgentWindowVisible?: boolean;
  systemSettingsWindowVisible?: boolean;
  windows?: AxWindow[];
  toggles?: AxToggle[];
  searchButtons?: AxSearchButton[];
  performed?: boolean;
};
function usage(): string {
  return [
    "Installed E11/E12 acceptance. It never runs unless --run is explicit.",
    "No-window safety checks:",
    "  bun scripts/incodex-e11-e12-installed-acceptance.ts --self-test",
    "Read-only preflight (provide the exact main ChatGPT PID):",
    "  bun scripts/incodex-e11-e12-installed-acceptance.ts --preflight --parent-pid PID",
    "Visible AX acceptance (only after the main agent independently verifies install, Runtime, signing, and ChatGPT Accessibility):",
    "  bun scripts/incodex-e11-e12-installed-acceptance.ts --run --parent-pid PID --out /private/tmp/new-e11-e12-dir",
    "The run uses exact-PID Accessibility actions and window metadata only; it captures no screenshots or page contents.",
  ].join("\n");
}

export function parseArguments(args: string[]): Options {
  let mode: Mode | null = null;
  let parentPid: number | undefined;
  let output: string | undefined;
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--preflight" || arg === "--run" || arg === "--self-test") {
      if (mode) throw new Error("choose exactly one of --preflight, --run, or --self-test");
      mode = arg === "--preflight" ? "preflight" : arg === "--run" ? "run" : "self-test";
    } else if (arg === "--parent-pid") {
      const raw = args[index + 1];
      const value = Number(raw);
      if (!raw || !Number.isSafeInteger(value) || value <= 0) throw new Error("--parent-pid requires a positive PID");
      parentPid = value;
      index += 1;
    } else if (arg === "--out") {
      const value = args[index + 1];
      if (!value || value.startsWith("--")) throw new Error("--out requires a new absolute output path");
      output = value;
      index += 1;
    } else if (arg === "--help" || arg === "-h") {
      throw new Error(usage());
    } else {
      throw new Error(`unknown argument: ${arg}`);
    }
  }
  if (!mode) throw new Error(usage());
  if (mode === "self-test" && (parentPid !== undefined || output !== undefined)) {
    throw new Error("--self-test does not accept --parent-pid or --out");
  }
  if ((mode === "run" || mode === "preflight") && !parentPid) {
    throw new Error(`--${mode} requires --parent-pid PID`);
  }
  if (mode === "preflight" && output !== undefined) throw new Error("--preflight does not accept --out");
  if (mode === "run") {
    if (!output || !isAbsolute(output)) throw new Error("--run requires --out with a new absolute path");
  }
  return { mode, parentPid, output };
}

export function newSessionNames(before: Iterable<string>, after: Iterable<string>): string[] {
  const previous = new Set(before);
  return [...new Set(after)].filter((name) => sessionName.test(name) && !previous.has(name)).sort();
}

export function parentChainTo(pid: number, expectedParentPid: number, parentOf: (processId: number) => number | null): number[] | null {
  let current = pid;
  const seen = new Set<number>([pid]);
  const chain = [pid];
  for (let depth = 0; depth < 32; depth += 1) {
    const parent = parentOf(current);
    if (parent === null || parent <= 1 || seen.has(parent)) return null;
    chain.push(parent);
    if (parent === expectedParentPid) return chain;
    seen.add(parent);
    current = parent;
  }
  return null;
}

export function hasParentInChain(pid: number, expectedParentPid: number, parentOf: (processId: number) => number | null): boolean {
  return parentChainTo(pid, expectedParentPid, parentOf) !== null;
}

export function exactNewPidSetIsSafe(currentPids: number[], baselinePids: number[], ownerPid: number): boolean {
  const baseline = new Set(baselinePids);
  const added = [...new Set(currentPids)].filter((pid) => !baseline.has(pid)).sort((a, b) => a - b);
  return added.length === 1 && added[0] === ownerPid && !baseline.has(ownerPid);
}

export function parseNewOwnerMetadata(raw: string, expectedSessionId: string): OwnerBinding {
  const value = JSON.parse(raw) as Record<string, unknown>;
  if (value.sessionId !== expectedSessionId) throw new Error("new session owner metadata has a mismatched sessionId");
  if (!Number.isSafeInteger(value.pid) || Number(value.pid) <= 0) throw new Error("new session owner metadata has no valid PID");
  if (typeof value.processStartIdentity !== "string" || !processStartIdentityPattern.test(value.processStartIdentity.trim().replace(/\s+/gu, " "))) {
    throw new Error("new session owner metadata has no canonical process-start identity");
  }
  return {
    sessionId: expectedSessionId,
    pid: Number(value.pid),
    processStartIdentity: value.processStartIdentity.trim().replace(/\s+/gu, " "),
  };
}

export function toggleFailures(toggle: AxToggle, expectedKind: "open" | "exit", searchBounds: Bounds): string[] {
  const failures: string[] = [];
  if (toggle.kind !== expectedKind) failures.push(`expected ${expectedKind} toggle, saw ${toggle.kind}`);
  if (toggle.role !== "AXCheckBox") failures.push(`expected AXCheckBox, saw ${toggle.role}`);
  const dimensions = [toggle.bounds.width, toggle.bounds.height, searchBounds.width, searchBounds.height];
  if (dimensions.some((dimension) => !Number.isFinite(dimension) || dimension <= 0)
    || Math.abs(toggle.bounds.width - searchBounds.width) > 0.25
    || Math.abs(toggle.bounds.height - searchBounds.height) > 0.25) {
    failures.push(`toggle size ${toggle.bounds.width}×${toggle.bounds.height} pt does not match live Search ${searchBounds.width}×${searchBounds.height} pt`);
  }
  if (!toggle.enabled) failures.push("toggle is disabled");
  if (!toggle.actions.includes("AXPress")) failures.push("toggle does not expose AXPress");
  return failures;
}

function runReadOnly(command: string, args: string[], timeout = 30_000): string {
  const result = spawnSync(command, args, { encoding: "utf8", timeout, env: { ...process.env, LC_ALL: "C" } });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${basename(command)} ${args.join(" ")} exited ${result.status}: ${(result.stderr || result.stdout).trim()}`);
  return result.stdout;
}

function sessionNames(): string[] {
  try {
    const rootStat = lstatSync(targetSessions);
    if (!rootStat.isDirectory() || rootStat.isSymbolicLink()) throw new Error("target session path is not a real directory");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return [];
    throw error;
  }
  return readdirSync(targetSessions, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && sessionName.test(entry.name))
    .map((entry) => entry.name)
    .sort();
}

function readOwnerBindingForNewSession(sessionId: string): OwnerBinding | null {
  const sessionRoot = join(targetSessions, sessionId);
  const rootStat = lstatSync(sessionRoot);
  if (!rootStat.isDirectory() || rootStat.isSymbolicLink()) throw new Error("new session root is not a real directory");
  const ownerPath = join(sessionRoot, "owner.json");
  let ownerFd: number | undefined;
  try {
    if (typeof fsConstants.O_NOFOLLOW !== "number") throw new Error("O_NOFOLLOW is unavailable; refusing to read owner metadata without symlink protection");
    ownerFd = openSync(ownerPath, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);
    const ownerStat = fstatSync(ownerFd);
    if (!ownerStat.isFile()) throw new Error("new owner metadata is not a regular file");
    // Read only this run's owner binding via a no-follow file descriptor. No profile, chat,
    // Chromium, or pre-existing session files are opened. Node's path-based open has no openat
    // equivalent here, so a hostile parent-directory replacement race cannot be made fully atomic.
    const raw = readFileSync(ownerFd, "utf8");
    const rootAfter = lstatSync(sessionRoot);
    if (!rootAfter.isDirectory() || rootAfter.isSymbolicLink() || rootAfter.dev !== rootStat.dev || rootAfter.ino !== rootStat.ino) {
      throw new Error("new session root identity changed while reading owner metadata");
    }
    return parseNewOwnerMetadata(raw, sessionId);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw error;
  } finally {
    if (ownerFd !== undefined) closeSync(ownerFd);
  }
}

function processStartIdentity(pid: number): string {
  return runReadOnly("/bin/ps", ["-p", String(pid), "-o", "lstart="]).trim().replace(/\s+/gu, " ");
}

function processParentPid(pid: number): number | null {
  try {
    const value = Number(runReadOnly("/bin/ps", ["-p", String(pid), "-o", "ppid="]).trim());
    return Number.isSafeInteger(value) && value > 0 ? value : null;
  } catch {
    return null;
  }
}

function isDescendantOf(pid: number, ancestorPid: number): boolean {
  return hasParentInChain(pid, ancestorPid, processParentPid);
}

function processParentChainTo(pid: number, ancestorPid: number): number[] | null {
  return parentChainTo(pid, ancestorPid, processParentPid);
}

function sameOwner(left: OwnerBinding | null, right: OwnerBinding): boolean {
  return left !== null && left.sessionId === right.sessionId && left.pid === right.pid && left.processStartIdentity === right.processStartIdentity;
}

function assertCurrentOwnerBinding(binding: OwnerBinding, binary: string, parentPid: number): void {
  const owner = readOwnerBindingForNewSession(binding.sessionId);
  if (!sameOwner(owner, binding)) throw new Error(`session ${binding.sessionId} no longer has the exact owner binding selected by this run`);
  if (processStartIdentity(binding.pid) !== binding.processStartIdentity) throw new Error(`PID ${binding.pid} process-start identity changed; refusing to address a reused PID`);
  const pids = axCall(binary, "processes", [executablePath]).matchingPids ?? [];
  if (!pids.includes(binding.pid)) throw new Error(`bound child PID ${binding.pid} is no longer the exact target executable`);
  if (!isDescendantOf(binding.pid, parentPid)) throw new Error(`bound child PID ${binding.pid} no longer has parent-chain evidence to main PID ${parentPid}`);
}

function assertExactProcessSet(binary: string, expectedPids: number[], reason: string): number[] {
  const pids = axCall(binary, "processes", [executablePath]).matchingPids ?? [];
  const expected = [...new Set(expectedPids)].sort((a, b) => a - b);
  const actual = [...new Set(pids)].sort((a, b) => a - b);
  if (actual.length !== expected.length || actual.some((pid, index) => pid !== expected[index])) {
    throw new Error(`${reason}: exact-path ChatGPT PID set changed; expected=${expected.join(",") || "none"}; actual=${actual.join(",") || "none"}`);
  }
  return actual;
}

function compileHelper(directory: string): string {
  const binary = join(directory, "incodex-e11-e12-ax");
  const result = spawnSync("/usr/bin/swiftc", ["-O", helperSource, "-o", binary], { encoding: "utf8", timeout: 120_000 });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Swift AX helper did not compile: ${(result.stderr || result.stdout).trim()}`);
  return binary;
}

function axCall(binary: string, command: string, args: string[] = [], timeout = 30_000): HelperReply {
  const result = spawnSync(binary, [command, ...args], { encoding: "utf8", timeout });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`AX helper ${command} failed: ${(result.stderr || result.stdout).trim()}`);
  const reply = JSON.parse(result.stdout) as HelperReply;
  if (!reply.ok) throw new Error(reply.error || `AX helper ${command} returned not-ok`);
  return reply;
}

function verifyScriptPreflight(parentPid: number, binary: string): { matchingPids: number[]; frontmost: HelperReply; sessionNames: string[] } {
  const matchingPids = axCall(binary, "processes", [executablePath]).matchingPids ?? [];
  if (!matchingPids.includes(parentPid)) throw new Error(`parent PID ${parentPid} is not the exact running ChatGPT executable`);
  const others = matchingPids.filter((pid) => pid !== parentPid);
  if (others.length) throw new Error(`pre-existing exact ChatGPT process(es) are excluded; refusing shared session state: ${others.join(", ")}`);
  const frontmost = axCall(binary, "frontmost");
  if (frontmost.securityAgentActive || frontmost.securityAgentWindowVisible) {
    throw new Error("a SecurityAgent/Keychain window is active or visible; no UI action may run until it is gone");
  }
  if (frontmost.systemSettingsActive || frontmost.systemSettingsWindowVisible) {
    throw new Error("a System Settings window is active or visible; no UI action may run until it is gone");
  }
  return { matchingPids, frontmost, sessionNames: sessionNames() };
}

function assertSystemWindowStateClear(state: HelperReply): void {
  if (state.securityAgentActive || state.securityAgentWindowVisible) {
    throw new Error("SecurityAgent/Keychain window appeared during acceptance; stopping without further AX actions");
  }
  if (state.systemSettingsActive || state.systemSettingsWindowVisible) {
    throw new Error("a System Settings window appeared during acceptance; stopping without further AX actions");
  }
}

function assertNoBlockingSystemWindow(binary: string): HelperReply {
  const state = axCall(binary, "frontmost");
  assertSystemWindowStateClear(state);
  return state;
}

function oneMainWindow(reply: HelperReply): AxWindow {
  const candidates = (reply.windows ?? []).filter((window) => window.layer === 0 && window.alpha > 0 && !window.isMinimized);
  const main = candidates.filter((window) => window.isMain);
  const selected = main.length === 1 ? main : candidates.length === 1 ? candidates : [];
  if (selected.length !== 1) throw new Error(`expected one visible primary layer-0 window; found ${candidates.length} candidates and ${main.length} main windows`);
  return selected[0];
}

function oneToggle(reply: HelperReply, kind: "open" | "exit", windowId: number): AxToggle {
  const matches = (reply.toggles ?? []).filter((toggle) => toggle.kind === kind && toggle.windowId === windowId);
  if (matches.length !== 1) throw new Error(`expected one ${kind} AXCheckBox in CG window ${windowId}; found ${matches.length}`);
  const toggle = matches[0];
  const searchButton = oneSearchButton(reply, windowId);
  const failures = toggleFailures(toggle, kind, searchButton.bounds);
  if (failures.length) throw new Error(failures.join("; "));
  return toggle;
}

function oneSearchButton(reply: HelperReply, windowId: number): AxSearchButton {
  const matches = (reply.searchButtons ?? []).filter((button) =>
    button.windowId === windowId
    && button.role === "AXButton"
    && SEARCH_LABELS.has(button.label.trim()));
  if (matches.length !== 1) throw new Error(`expected one official Search AXButton in CG window ${windowId}; found ${matches.length}`);
  return matches[0];
}

function compareBounds(source: Bounds, child: Bounds): BoundsComparison {
  const dx = child.x - source.x;
  const dy = child.y - source.y;
  return {
    source,
    child,
    delta: { x: dx, y: dy },
    sameSize: Math.abs(source.width - child.width) <= 1 && Math.abs(source.height - child.height) <= 1,
    chromeTile22: Math.abs(dx - 22) <= 1 && Math.abs(dy - 22) <= 1,
  };
}

function requireExpectedTile(source: Bounds, child: Bounds, description: string): BoundsComparison {
  const comparison = compareBounds(source, child);
  if (!comparison.sameSize || !comparison.chromeTile22) {
    throw new Error(`${description} did not match the expected 22 pt child-window tile with unchanged size; refusing to bind or close it`);
  }
  return comparison;
}

async function waitFor<T>(label: string, read: () => T | null, predicate: (value: T) => boolean, timeoutMs: number): Promise<T> {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const value = read();
    if (value !== null && predicate(value)) return value;
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 200));
  }
  throw new Error(`timed out waiting for ${label} after ${timeoutMs}ms`);
}

function newRootsFrom(before: string[]): string[] {
  return newSessionNames(before, sessionNames());
}

async function waitForNewOwner(before: string[], binary: string, baselinePids: number[], parentPid: number, timeoutMs = 120_000): Promise<OwnerBinding> {
  return waitFor("one new session root with handed-off owner metadata", () => {
    assertNoBlockingSystemWindow(binary);
    const added = newRootsFrom(before);
    if (added.length > 1) throw new Error(`more than one new session root appeared: ${added.join(", ")}`);
    const processes = axCall(binary, "processes", [executablePath]).matchingPids ?? [];
    const addedPids = [...new Set(processes)].filter((pid) => !baselinePids.includes(pid));
    if (addedPids.length > 1) throw new Error(`multiple unexpected exact-path ChatGPT PIDs appeared after the hat trigger: ${addedPids.join(", ")}`);
    for (const pid of addedPids) {
      if (!isDescendantOf(pid, parentPid)) throw new Error(`new exact-path ChatGPT PID ${pid} has no parent-chain link to triggered main PID ${parentPid}; refusing to bind or clean it up`);
    }
    if (added.length === 0) return null;
    const owner = readOwnerBindingForNewSession(added[0]);
    if (!owner) return null;
    if (baselinePids.includes(owner.pid)) throw new Error(`new session ${owner.sessionId} points to pre-existing PID ${owner.pid}; refusing ambiguous ownership`);
    if (!processes.includes(owner.pid)) return null;
    if (!exactNewPidSetIsSafe(processes, baselinePids, owner.pid)) {
      throw new Error(`new session owner PID ${owner.pid} does not uniquely match the exact-path PID delta; refusing ambiguous ownership`);
    }
    const parentChainPids = processParentChainTo(owner.pid, parentPid);
    if (!parentChainPids) throw new Error(`new session owner PID ${owner.pid} has no parent-chain link to triggered main PID ${parentPid}; refusing to bind or clean it up`);
    if (processStartIdentity(owner.pid) !== owner.processStartIdentity) throw new Error(`new session PID ${owner.pid} process-start identity does not match owner metadata`);
    return { ...owner, parentChainPids };
  }, (owner) => owner.pid > 0, timeoutMs);
}

async function waitForBurn(binding: OwnerBinding, binary: string, parentPid: number, timeoutMs = 120_000): Promise<void> {
  await waitFor(`session ${binding.sessionId} root and PID ${binding.pid} to disappear`, () => {
    assertNoBlockingSystemWindow(binary);
    let rootExists = true;
    try { lstatSync(join(targetSessions, binding.sessionId)); } catch (error) {
      if ((error as NodeJS.ErrnoException).code === "ENOENT") rootExists = false;
      else throw error;
    }
    const processes = axCall(binary, "processes", [executablePath]).matchingPids ?? [];
    const sameOwnerStillAlive = processes.includes(binding.pid) && processStartIdentity(binding.pid) === binding.processStartIdentity;
    if (rootExists) {
      const currentOwner = readOwnerBindingForNewSession(binding.sessionId);
      if (currentOwner && !sameOwner(currentOwner, binding)) throw new Error(`session ${binding.sessionId} owner metadata changed while waiting for close`);
    }
    const unexpectedPids = processes.filter((pid) => pid !== parentPid && !(pid === binding.pid && sameOwnerStillAlive));
    if (unexpectedPids.length) throw new Error(`unexpected exact-path ChatGPT PID(s) appeared while waiting for close: ${unexpectedPids.join(", ")}`);
    return { rootExists, childAlive: sameOwnerStillAlive, parentAlive: processes.includes(parentPid) };
  }, (state) => !state.rootExists && !state.childAlive && state.parentAlive, timeoutMs);
}

function assertSessionNamesRestored(before: string[]): void {
  const after = sessionNames();
  const added = newSessionNames(before, after);
  const removed = before.filter((name) => !after.includes(name));
  if (added.length || removed.length) {
    throw new Error(`session-root set changed after test; added=${added.join(",") || "none"}; removed=${removed.join(",") || "none"}`);
  }
}

function callToggle(binary: string, pid: number, kind: "open" | "exit", windowId: number): AxToggle {
  const labels = JSON.stringify(labelsByKind);
  const inspected = axCall(binary, "inspect", [String(pid), executablePath, labels]);
  return oneToggle(inspected, kind, windowId);
}

async function inspectWhenWindowReady(binary: string, pid: number, timeoutMs = 10_000): Promise<HelperReply> {
  return waitFor(`AXWindow for exact PID ${pid}`, () => {
    assertNoBlockingSystemWindow(binary);
    try {
      return axCall(binary, "inspect", [String(pid), executablePath, JSON.stringify(labelsByKind)]);
    } catch (error) {
      // A just-spawned Electron process can exist before its native AXWindow.
      // Retry only that observed transient; every other AX failure stays fatal.
      if (String(error).includes("AX attribute AXWindows unavailable (AXError -25204)")) return null;
      throw error;
    }
  }, (reply) => (reply.windows?.length ?? 0) > 0, timeoutMs);
}

async function inspectWhenExitToggleReady(
  binary: string,
  binding: OwnerBinding,
  parentPid: number,
  parentBounds: Bounds,
  windowId: number,
  timeoutMs = 15_000,
): Promise<{ inspect: HelperReply; window: AxWindow; toggle: AxToggle }> {
  return waitFor(`private exit AXCheckBox in exact window ${windowId}`, () => {
    assertNoBlockingSystemWindow(binary);
    assertCurrentOwnerBinding(binding, binary, parentPid);
    const inspected = axCall(binary, "inspect", [String(binding.pid), executablePath, JSON.stringify(labelsByKind)]);
    const window = oneMainWindow(inspected);
    if (window.windowId !== windowId) throw new Error("private child main-window CGWindowID changed while awaiting renderer AX");
    requireExpectedTile(parentBounds, window.bounds, "private child window awaiting renderer AX");
    const matches = (inspected.toggles ?? []).filter((toggle) => toggle.kind === "exit" && toggle.windowId === windowId);
    if (matches.length > 1) throw new Error("multiple private exit AXCheckBoxes appeared in the exact child window");
    if (matches.length === 0) return null;
    return { inspect: inspected, window, toggle: oneToggle(inspected, "exit", windowId) };
  }, () => true, timeoutMs);
}

function activateThenPress(binary: string, pid: number, kind: "open" | "exit", windowId: number): AxToggle {
  axCall(binary, "activate", [String(pid), executablePath]);
  const toggle = callToggle(binary, pid, kind, windowId);
  axCall(binary, "press", [String(pid), executablePath, JSON.stringify(labelsByKind), kind, String(windowId)]);
  return toggle;
}

function appendStep(report: Record<string, unknown>, name: string, result: unknown): void {
  const steps = report.steps as Array<Record<string, unknown>>;
  steps.push({ name, result });
}

async function runAcceptance(parentPid: number, outputPath: string): Promise<void> {
  if (existsSync(outputPath)) throw new Error(`output path already exists: ${outputPath}`);
  const helperTemp = mkdtempSync(join(tmpdir(), "incodex-e11-e12-helper-"));
  let helper: string;
  let outputRealPath: string;
  let initialPreflight: ReturnType<typeof verifyScriptPreflight>;
  try {
    helper = compileHelper(helperTemp);
    initialPreflight = verifyScriptPreflight(parentPid, helper);
    mkdirSync(outputPath, { recursive: false, mode: 0o700 });
    outputRealPath = realpathSync(outputPath);
  } catch (error) {
    rmSync(helperTemp, { recursive: true, force: true });
    throw error;
  }
  const report: Record<string, unknown> = {
    schema: "incodex-e11-e12-installed-acceptance-v1",
    startedAt: new Date().toISOString(),
    productHead: runReadOnly("git", ["-C", root, "rev-parse", "--short", "HEAD"]).trim(),
    cliSha256: createHash("sha256").update(readFileSync(cli)).digest("hex"),
    target: appPath,
    bundleIdentifier,
    parentPid,
    targetId,
    existingSessionRootNames: initialPreflight.sessionNames,
    initialProcessPids: initialPreflight.matchingPids,
    initialFrontmostPid: initialPreflight.frontmost.frontmostPid,
    securityAgentWindowVisibleAtPreflight: initialPreflight.frontmost.securityAgentWindowVisible ?? false,
    systemSettingsWindowVisibleAtPreflight: initialPreflight.frontmost.systemSettingsWindowVisible ?? false,
    steps: [],
    limitation: "AX and path/process metadata only; no page text, chat/profile or pre-existing session-file reads, screenshots, TCC changes, or forced process termination. Main agent must independently verify installed package, Runtime, signature, and ChatGPT Accessibility before --run; doctor is not invoked here because it scans pre-existing sessions. Owner metadata has no action-correlation token, so binding is accepted only for one new root plus one new exact-path PID, matching owner PID/start identity, parent-chain evidence to the triggered main PID, and the expected 22 pt child tile; any ambiguity fails closed without cleanup. Owner metadata is opened with O_NOFOLLOW and checked against the session root inode before/after reading; Node path APIs do not provide atomic openat-relative parent resolution, so a hostile directory replacement race is not fully eliminated.",
    notCovered: [
      "E11 keyboard shortcut: this run does not synthesize keystrokes or request CGPreflightPostEventAccess; do not mark E11 fully closed from this script alone.",
      "E12 late recreation and descendant-process command-line reference scan are not covered; no user-data or process arguments are inspected.",
    ],
  };
  let activeBinding: OwnerBinding | null = null;
  let parentWindowForCleanup: AxWindow | null = null;
  let overallError: Error | null = null;
  try {
    const processesBefore = initialPreflight.matchingPids;
    const sessionBaseline = initialPreflight.sessionNames;
    const parentInspect = axCall(helper, "inspect", [String(parentPid), executablePath, JSON.stringify(labelsByKind)]);
    const parentWindow = oneMainWindow(parentInspect);
    parentWindowForCleanup = parentWindow;
    const openToggle = oneToggle(parentInspect, "open", parentWindow.windowId);
    const openSearchButton = oneSearchButton(parentInspect, parentWindow.windowId);
    appendStep(report, "E11 main window and live-Search-sized AXCheckBox", { toggle: openToggle, searchButton: openSearchButton, window: parentWindow });
    const parentFront = axCall(helper, "frontmost", []).frontmostPid;
    if (parentFront !== parentPid) throw new Error(`ChatGPT parent PID ${parentPid} is not frontmost; no control was pressed`);

    assertNoBlockingSystemWindow(helper);
    assertSessionNamesRestored(sessionBaseline);
    const firstProcessBaseline = assertExactProcessSet(helper, processesBefore, "before first hat trigger");
    const openedToggle = callToggle(helper, parentPid, "open", parentWindow.windowId);
    axCall(helper, "press", [String(parentPid), executablePath, JSON.stringify(labelsByKind), "open", String(parentWindow.windowId)]);
    const firstBinding = await waitForNewOwner(sessionBaseline, helper, firstProcessBaseline, parentPid);
    assertCurrentOwnerBinding(firstBinding, helper, parentPid);
    activeBinding = firstBinding;
    const childInspect = await inspectWhenWindowReady(helper, firstBinding.pid);
    const childWindow = oneMainWindow(childInspect);
    const firstBounds = requireExpectedTile(parentWindow.bounds, childWindow.bounds, "first private child window");
    const childExitState = await inspectWhenExitToggleReady(helper, firstBinding, parentPid, parentWindow.bounds, childWindow.windowId);
    const childToggle = childExitState.toggle;
    assertExactProcessSet(helper, [parentPid, firstBinding.pid], "after first hat trigger");
    appendStep(report, "E11/E12 first hat open", {
      toggle: openedToggle,
      sessionId: firstBinding.sessionId,
      childPid: firstBinding.pid,
      parentChainPids: firstBinding.parentChainPids ?? [],
      bounds: firstBounds,
      exitToggle: childToggle,
      exitSearchButton: oneSearchButton(childExitState.inspect, childWindow.windowId),
    });
    const parentAfterOpen = oneMainWindow(axCall(helper, "inspect", [String(parentPid), executablePath, JSON.stringify(labelsByKind)]));
    const parentAfterOpenBounds = compareBounds(parentWindow.bounds, parentAfterOpen.bounds);
    if (parentAfterOpen.windowId !== parentWindow.windowId || !parentAfterOpenBounds.sameSize || Math.abs(parentAfterOpenBounds.delta.x) > 1 || Math.abs(parentAfterOpenBounds.delta.y) > 1) {
      throw new Error("normal parent window identity or bounds changed while private child was open");
    }
    appendStep(report, "E12 normal parent window remains unchanged behind private child", { windowId: parentAfterOpen.windowId, bounds: parentAfterOpenBounds });

    assertCurrentOwnerBinding(firstBinding, helper, parentPid);
    assertExactProcessSet(helper, [parentPid, firstBinding.pid], "before repeated hat trigger");
    activateThenPress(helper, parentPid, "open", parentWindow.windowId);
    await waitFor("same private child to be raised after repeated hat trigger", () => {
      assertNoBlockingSystemWindow(helper);
      const names = sessionNames();
      const processes = axCall(helper!, "processes", [executablePath]).matchingPids ?? [];
      const newNames = newSessionNames(sessionBaseline, names);
      if (newNames.length > 1 || (newNames.length === 1 && newNames[0] !== firstBinding.sessionId)) {
        throw new Error(`unexpected new session root(s) appeared after repeated hat trigger: ${newNames.join(", ")}`);
      }
      if (!exactNewPidSetIsSafe(processes, processesBefore, firstBinding.pid)) {
        throw new Error("exact-path ChatGPT PID set changed during repeated hat trigger; refusing to attribute another session");
      }
      const frontmost = axCall(helper!, "frontmost", []).frontmostPid;
      return { names, processes, frontmost };
    }, (state) => {
      const newNames = newSessionNames(sessionBaseline, state.names);
      return newNames.length === 1 && newNames[0] === firstBinding.sessionId && state.processes.includes(firstBinding.pid) && state.frontmost === firstBinding.pid;
    }, 20_000);
    appendStep(report, "E12 repeated trigger reuses same session", {
      sessionId: firstBinding.sessionId,
      childPid: firstBinding.pid,
      newSessionRootCount: newRootsFrom(sessionBaseline).length,
    });

    assertCurrentOwnerBinding(firstBinding, helper, parentPid);
    assertExactProcessSet(helper, [parentPid, firstBinding.pid], "before hat close");
    const exitInspect = axCall(helper, "inspect", [String(firstBinding.pid), executablePath, JSON.stringify(labelsByKind)]);
    const currentChildWindow = oneMainWindow(exitInspect);
    if (currentChildWindow.windowId !== childWindow.windowId) throw new Error("private child main-window CGWindowID changed before hat close");
    requireExpectedTile(parentWindow.bounds, currentChildWindow.bounds, "private child window before hat close");
    const exitToggle = oneToggle(exitInspect, "exit", currentChildWindow.windowId);
    axCall(helper, "press", [String(firstBinding.pid), executablePath, JSON.stringify(labelsByKind), "exit", String(currentChildWindow.windowId)]);
    await waitForBurn(firstBinding, helper, parentPid);
    appendStep(report, "E12 private-window hat close burns session", {
      toggle: exitToggle,
      searchButton: oneSearchButton(exitInspect, currentChildWindow.windowId),
      sessionId: firstBinding.sessionId,
      childPid: firstBinding.pid,
      burned: true,
    });
    activeBinding = null;

    const secondBaseline = sessionNames();
    assertSessionNamesRestored(sessionBaseline);
    const secondProcessBaseline = assertExactProcessSet(helper, [parentPid], "before second hat trigger");
    const parentBeforeSecond = oneMainWindow(axCall(helper, "inspect", [String(parentPid), executablePath, JSON.stringify(labelsByKind)]));
    if (parentBeforeSecond.windowId !== parentWindow.windowId) throw new Error("normal parent main-window CGWindowID changed before second hat trigger");
    const parentSecondBounds = compareBounds(parentWindow.bounds, parentBeforeSecond.bounds);
    if (!parentSecondBounds.sameSize || Math.abs(parentSecondBounds.delta.x) > 1 || Math.abs(parentSecondBounds.delta.y) > 1) {
      throw new Error("normal parent window bounds changed before second hat trigger");
    }
    activateThenPress(helper, parentPid, "open", parentWindow.windowId);
    const secondBinding = await waitForNewOwner(secondBaseline, helper, secondProcessBaseline, parentPid);
    assertCurrentOwnerBinding(secondBinding, helper, parentPid);
    activeBinding = secondBinding;
    const secondInspect = await inspectWhenWindowReady(helper, secondBinding.pid);
    const secondWindow = oneMainWindow(secondInspect);
    const secondBounds = requireExpectedTile(parentWindow.bounds, secondWindow.bounds, "second private child window");
    const secondExitState = await inspectWhenExitToggleReady(helper, secondBinding, parentPid, parentWindow.bounds, secondWindow.windowId);
    const secondToggle = secondExitState.toggle;
    assertExactProcessSet(helper, [parentPid, secondBinding.pid], "after second hat trigger");
    appendStep(report, "E12 native-close session opened", {
      toggle: secondToggle,
      searchButton: oneSearchButton(secondExitState.inspect, secondWindow.windowId),
      sessionId: secondBinding.sessionId,
      childPid: secondBinding.pid,
      parentChainPids: secondBinding.parentChainPids ?? [],
      bounds: secondBounds,
    });
    assertCurrentOwnerBinding(secondBinding, helper, parentPid);
    const currentSecondInspect = axCall(helper, "inspect", [String(secondBinding.pid), executablePath, JSON.stringify(labelsByKind)]);
    const currentSecondWindow = oneMainWindow(currentSecondInspect);
    if (currentSecondWindow.windowId !== secondWindow.windowId) throw new Error("private child main-window CGWindowID changed before native close");
    requireExpectedTile(parentWindow.bounds, currentSecondWindow.bounds, "private child window before native close");
    axCall(helper, "close-window", [String(secondBinding.pid), executablePath, String(secondWindow.windowId)]);
    await waitForBurn(secondBinding, helper, parentPid);
    appendStep(report, "E12 exact native window close burns session", { windowId: secondWindow.windowId, sessionId: secondBinding.sessionId, childPid: secondBinding.pid, burned: true });
    activeBinding = null;
    assertSessionNamesRestored(sessionBaseline);
    appendStep(report, "pre-existing session roots unchanged", { rootNames: sessionBaseline });
    const finalProcesses = axCall(helper, "processes", [executablePath]).matchingPids ?? [];
    if (!finalProcesses.includes(parentPid)) throw new Error("normal ChatGPT parent exited unexpectedly");
    appendStep(report, "normal ChatGPT parent remains alive", { parentPid, matchingPids: finalProcesses });
    const finalParentWindow = oneMainWindow(axCall(helper, "inspect", [String(parentPid), executablePath, JSON.stringify(labelsByKind)]));
    const finalParentBounds = compareBounds(parentWindow.bounds, finalParentWindow.bounds);
    if (finalParentWindow.windowId !== parentWindow.windowId || !finalParentBounds.sameSize || Math.abs(finalParentBounds.delta.x) > 1 || Math.abs(finalParentBounds.delta.y) > 1) {
      throw new Error("normal parent window did not return with the same identity and bounds after both child close paths");
    }
    appendStep(report, "normal parent window identity and bounds restored", { windowId: finalParentWindow.windowId, bounds: finalParentBounds });
  } catch (error) {
    overallError = error instanceof Error ? error : new Error(String(error));
    (report.steps as Array<Record<string, unknown>>).push({ name: "run-error", error: overallError.message });
    const pendingCleanup = activeBinding;
    if (pendingCleanup && helper) {
      try {
        assertNoBlockingSystemWindow(helper);
        assertCurrentOwnerBinding(pendingCleanup, helper, parentPid);
        assertExactProcessSet(helper, [parentPid, pendingCleanup.pid], "before failure cleanup");
        if (axCall(helper, "frontmost", []).frontmostPid === pendingCleanup.pid) {
          const cleanupParentWindow = oneMainWindow(axCall(helper, "inspect", [String(parentPid), executablePath, JSON.stringify(labelsByKind)]));
          if (parentWindowForCleanup && cleanupParentWindow.windowId !== parentWindowForCleanup.windowId) {
            throw new Error("parent window identity changed; refusing failure cleanup");
          }
          const cleanupInspect = await inspectWhenWindowReady(helper, pendingCleanup.pid);
          const cleanupChildWindow = oneMainWindow(cleanupInspect);
          requireExpectedTile(cleanupParentWindow.bounds, cleanupChildWindow.bounds, "private child window before failure cleanup");
          const exitToggles = (cleanupInspect.toggles ?? []).filter((toggle) => toggle.kind === "exit" && toggle.windowId === cleanupChildWindow.windowId);
          if (exitToggles.length > 1) throw new Error("multiple private exit toggles; refusing failure cleanup");
          assertCurrentOwnerBinding(pendingCleanup, helper, parentPid);
          if (exitToggles.length === 1) {
            oneToggle(cleanupInspect, "exit", cleanupChildWindow.windowId);
            axCall(helper, "press", [String(pendingCleanup.pid), executablePath, JSON.stringify(labelsByKind), "exit", String(cleanupChildWindow.windowId)]);
          } else {
            // Renderer AX can be disabled even while the native child window is
            // present. Close only this identity-checked test window via AppKit.
            axCall(helper, "close-window", [String(pendingCleanup.pid), executablePath, String(cleanupChildWindow.windowId)]);
          }
          await waitForBurn(pendingCleanup, helper, parentPid, 30_000);
          (report.steps as Array<Record<string, unknown>>).push({ name: "safe exact-window cleanup after failed assertion", sessionId: pendingCleanup.sessionId, childPid: pendingCleanup.pid, method: exitToggles.length === 1 ? "product exit toggle" : "native close button (renderer AX unavailable)", burned: true });
          activeBinding = null;
        } else {
          (report.steps as Array<Record<string, unknown>>).push({ name: "cleanup not attempted", sessionId: pendingCleanup.sessionId, childPid: pendingCleanup.pid, reason: "child not frontmost; preserving unrelated/system UI and process" });
        }
      } catch (cleanupError) {
        (report.steps as Array<Record<string, unknown>>).push({ name: "cleanup needs manual review", sessionId: pendingCleanup.sessionId, childPid: pendingCleanup.pid, error: String(cleanupError) });
      }
    }
  } finally {
    report.finishedAt = new Date().toISOString();
    report.ok = overallError === null;
    try {
      writeFileSync(join(outputRealPath, "e11-e12.json"), `${JSON.stringify(report, null, 2)}\n`, { flag: "wx", mode: 0o600 });
    } finally {
      rmSync(helperTemp, { recursive: true, force: true });
    }
  }
  if (overallError) throw overallError;
  process.stdout.write(`${JSON.stringify({ ok: true, evidence: join(outputRealPath, "e11-e12.json") }, null, 2)}\n`);
}

function preflight(parentPid: number): void {
  const helperTemp = mkdtempSync(join(tmpdir(), "incodex-e11-e12-preflight-"));
  try {
    const helper = compileHelper(helperTemp);
    const checked = verifyScriptPreflight(parentPid, helper);
    process.stdout.write(`${JSON.stringify({
      ok: true,
      mode: "read-only-process-modal-preflight",
      target: appPath,
      bundleIdentifier,
      parentPid,
      matchingPids: checked.matchingPids,
      frontmostPid: checked.frontmost.frontmostPid,
      securityAgentWindowVisible: checked.frontmost.securityAgentWindowVisible ?? false,
      systemSettingsWindowVisible: checked.frontmost.systemSettingsWindowVisible ?? false,
      targetId,
      existingSessionRootNames: checked.sessionNames,
      installGate: "not checked here: main agent must independently verify package, Runtime, signature, Accessibility and interrupted transactions; doctor is intentionally not invoked because it scans old sessions",
      uiTouched: false,
      existingSessionFilesRead: false,
    }, null, 2)}\n`);
  } finally {
    rmSync(helperTemp, { recursive: true, force: true });
  }
}

function selfTest(): void {
  assert.deepEqual(newSessionNames(["s-z5iJ1p", "notes", ".incodex-burned-old"], ["s-z5iJ1p", "notes", ".incodex-burned-old", "s-NewSession9"]), ["s-NewSession9"]);
  assert.deepEqual(parentChainTo(30, 10, (pid) => new Map([[30, 20], [20, 10]]).get(pid) ?? null), [30, 20, 10]);
  assert.equal(parentChainTo(30, 10, (pid) => new Map([[30, 40], [40, 1]]).get(pid) ?? null), null);
  assert.equal(hasParentInChain(30, 10, (pid) => new Map([[30, 20], [20, 10]]).get(pid) ?? null), true);
  assert.equal(hasParentInChain(30, 10, (pid) => new Map([[30, 40], [40, 1]]).get(pid) ?? null), false);
  assert.equal(hasParentInChain(30, 10, (pid) => new Map([[30, 40], [40, 30]]).get(pid) ?? null), false);
  assert.equal(exactNewPidSetIsSafe([456, 123], [123], 456), true);
  assert.equal(exactNewPidSetIsSafe([456, 789, 123], [123], 456), false);
  assert.equal(exactNewPidSetIsSafe([123], [123], 123), false);
  assert.deepEqual(parseNewOwnerMetadata('{"sessionId":"s-NewSession9","pid":456,"processStartIdentity":"Thu Jan 1 00:00:00 1970","unrelated":"must not escape"}', "s-NewSession9"), {
    sessionId: "s-NewSession9", pid: 456, processStartIdentity: "Thu Jan 1 00:00:00 1970",
  });
  assert.throws(() => parseNewOwnerMetadata('{"sessionId":"s-other","pid":456,"processStartIdentity":"Thu Jan 1 00:00:00 1970"}', "s-NewSession9"), /mismatched sessionId/u);
  assert.throws(() => parseNewOwnerMetadata('{"sessionId":"s-NewSession9","pid":0,"processStartIdentity":"Thu Jan 1 00:00:00 1970"}', "s-NewSession9"), /valid PID/u);
  assert.throws(() => parseNewOwnerMetadata('{"sessionId":"s-NewSession9","pid":456,"processStartIdentity":"invalid"}', "s-NewSession9"), /canonical process-start identity/u);
  assert.doesNotThrow(() => assertSystemWindowStateClear({ ok: true, securityAgentActive: false, securityAgentWindowVisible: false, systemSettingsActive: false }));
  assert.throws(() => assertSystemWindowStateClear({ ok: true, securityAgentActive: false, securityAgentWindowVisible: true }), /SecurityAgent\/Keychain/u);
  assert.throws(() => assertSystemWindowStateClear({ ok: true, securityAgentActive: true, securityAgentWindowVisible: false }), /SecurityAgent\/Keychain/u);
  assert.throws(() => assertSystemWindowStateClear({ ok: true, systemSettingsActive: true }), /System Settings/u);
  assert.throws(() => assertSystemWindowStateClear({ ok: true, systemSettingsActive: false, systemSettingsWindowVisible: true }), /System Settings/u);
  const validToggle: AxToggle = {
    role: "AXCheckBox", label: "Open incognito window", kind: "open", enabled: true,
    value: false, bounds: { x: 1, y: 2, width: 24, height: 24 }, windowId: 99,
    windowBounds: { x: 0, y: 0, width: 800, height: 600 }, actions: ["AXPress"],
  };
  const liveSearch24: AxSearchButton = { role: "AXButton", label: "Search", bounds: { x: 40, y: 2, width: 24, height: 24 }, windowId: 99 };
  const liveSearch28 = { role: "AXButton", label: "Search", bounds: { x: 40, y: 2, width: 28, height: 28 }, windowId: 99 };
  const validToggle28 = { ...validToggle, bounds: { ...validToggle.bounds, width: 28, height: 28 } };
  assert.deepEqual(toggleFailures(validToggle, "open", liveSearch24.bounds), []);
  assert.deepEqual(toggleFailures(validToggle28, "open", liveSearch28.bounds), []);
  assert.deepEqual(oneToggle({ ok: true, toggles: [validToggle, { ...validToggle, windowId: 100 }], searchButtons: [liveSearch24, { ...liveSearch24, windowId: 100 }] }, "open", 99), validToggle);
  assert.deepEqual(oneToggle({ ok: true, toggles: [validToggle28], searchButtons: [liveSearch28] }, "open", 99), validToggle28);
  assert.throws(() => oneToggle({ ok: true, toggles: [{ ...validToggle, windowId: 100 }], searchButtons: [liveSearch24] }, "open", 99), /CG window 99/u);
  assert.throws(() => oneToggle({ ok: true, toggles: [validToggle, validToggle], searchButtons: [liveSearch24] }, "open", 99), /found 2/u);
  assert.throws(() => oneToggle({ ok: true, toggles: [{ ...validToggle, bounds: { ...validToggle.bounds, width: 28, height: 28 } }], searchButtons: [liveSearch24] }, "open", 99), /does not match live Search/u);
  assert.throws(() => oneToggle({ ok: true, toggles: [validToggle] }, "open", 99), /official Search AXButton.*found 0/u);
  assert.throws(() => oneToggle({ ok: true, toggles: [validToggle], searchButtons: [liveSearch24, liveSearch24] }, "open", 99), /official Search AXButton.*found 2/u);
  assert.throws(() => oneToggle({ ok: true, toggles: [validToggle], searchButtons: [{ ...liveSearch24, windowId: 100 }] }, "open", 99), /official Search AXButton.*found 0/u);
  assert.throws(() => oneToggle({ ok: true, toggles: [validToggle], searchButtons: [{ ...liveSearch24, role: "AXTextField" }] }, "open", 99), /official Search AXButton.*found 0/u);
  assert.throws(() => oneToggle({ ok: true, toggles: [validToggle], searchButtons: [{ ...liveSearch24, label: "Not Search" }] }, "open", 99), /official Search AXButton.*found 0/u);
  assert.deepEqual(requireExpectedTile({ x: 100, y: 200, width: 600, height: 400 }, { x: 122, y: 222, width: 600, height: 400 }, "fixture"), {
    source: { x: 100, y: 200, width: 600, height: 400 },
    child: { x: 122, y: 222, width: 600, height: 400 },
    delta: { x: 22, y: 22 },
    sameSize: true,
    chromeTile22: true,
  });
  assert.throws(() => requireExpectedTile({ x: 100, y: 200, width: 600, height: 400 }, { x: 124, y: 222, width: 600, height: 400 }, "fixture"), /22 pt child-window tile/u);
  assert(toggleFailures({ ...validToggle, role: "AXButton" }, "open", liveSearch24.bounds).some((failure) => failure.includes("AXCheckBox")));
  assert(toggleFailures({ ...validToggle, bounds: { ...validToggle.bounds, height: 20 } }, "open", liveSearch24.bounds).some((failure) => failure.includes("live Search")));
  assert(toggleFailures({ ...validToggle, enabled: false }, "open", liveSearch24.bounds).includes("toggle is disabled"));
  assert(toggleFailures({ ...validToggle, actions: [] }, "open", liveSearch24.bounds).includes("toggle does not expose AXPress"));
  assert.deepEqual(parseArguments(["--preflight", "--parent-pid", "123"]), { mode: "preflight", parentPid: 123, output: undefined });
  assert.throws(() => parseArguments(["--run"]), /requires --parent-pid/u);
  assert.throws(() => parseArguments(["--self-test", "--parent-pid", "123"]), /does not accept/u);
  const helperTemp = mkdtempSync(join(tmpdir(), "incodex-e11-e12-self-test-"));
  let swiftHelperSelfTest: Record<string, unknown>;
  try {
    const helper = compileHelper(helperTemp);
    const result = spawnSync(helper, ["self-test"], { encoding: "utf8", timeout: 30_000 });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(`Swift AX helper self-test failed: ${(result.stderr || result.stdout).trim()}`);
    swiftHelperSelfTest = JSON.parse(result.stdout) as Record<string, unknown>;
    assert.equal(swiftHelperSelfTest.ok, true);
  } finally {
    rmSync(helperTemp, { recursive: true, force: true });
  }
  process.stdout.write(`${JSON.stringify({ ok: true, mode: "no-window-self-test", uiTouched: false, swiftHelperSelfTest })}\n`);
}

async function main(): Promise<void> {
  const options = parseArguments(Bun.argv.slice(2));
  if (process.platform !== "darwin") throw new Error("this acceptance script is macOS-only");
  if (options.mode === "self-test") selfTest();
  else if (options.mode === "preflight") preflight(options.parentPid!);
  else await runAcceptance(options.parentPid!, options.output!);
}

if (import.meta.main) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
