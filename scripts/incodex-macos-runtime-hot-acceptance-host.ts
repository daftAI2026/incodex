import { randomUUID } from "node:crypto";
import { chmodSync, closeSync, constants as fsConstants, fstatSync, lstatSync, mkdirSync, openSync, readFileSync, realpathSync, readdirSync, renameSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { basename, isAbsolute, join } from "node:path";
import { spawnSync } from "node:child_process";
import { createServer } from "node:net";
import {
  ACK_LOG, ACK_TIMEOUT_MS, CLOSE_TIMEOUT_MS, CHILD_TIMEOUT_MS, MAX_LOG_BYTES,
  POLL_MS, PRODUCT_APP, PRODUCT_BINARY, SESSION_ID, SESSIONS_ROOT,
  STARTUP_TIMEOUT_MS, TARGET_STATE,
  acceptedRuntimeAck, isRecord, readCandidateStatus, readCurrentPointer,
  requireBundledIdentity, requireCurrentCandidate, runReadOnly, verifyActionResult, verifyCliVersion,
} from "./incodex-macos-runtime-hot-acceptance-contract.ts";
const AX_SOURCE = join(import.meta.dir, "incodex-e11-e12-installed-ax.swift");

import type { AckExpectation, ActionExpectation, CandidateEvidence, Report, RuntimeAck } from "./incodex-macos-runtime-hot-acceptance-contract.ts";

export function officialHostPids(): number[] {
  const names = runReadOnly("/bin/ps", ["-axo", "pid=,comm="], 10_000);
  const commands = runReadOnly("/bin/ps", ["-axo", "pid=,command="], 10_000);
  const pids = new Set<number>();
  for (const line of names.split(/\r?\n/u)) {
    const match = line.match(/^\s*(\d+)\s+(.+)$/u);
    if (match && basename(match[2]!.trim()) === "ChatGPT") pids.add(Number(match[1]));
  }
  for (const line of commands.split(/\r?\n/u)) {
    const match = line.match(/^\s*(\d+)\s+(.+)$/u);
    if (match && (match[2] === PRODUCT_BINARY || match[2]!.startsWith(`${PRODUCT_BINARY} `))) pids.add(Number(match[1]));
  }
  return [...pids].sort((left, right) => left - right);
}

export function requireNoOfficialHost(): void {
  if (officialHostPids().length !== 0) throw new Error("an official ChatGPT host is already running; it was not touched");
}

export function processStartIdentity(pid: number): string | null {
  const result = spawnSync("/bin/ps", ["-p", String(pid), "-o", "pid=,lstart=,comm="], {
    encoding: "utf8", timeout: 5_000, env: { ...process.env, LC_ALL: "C" },
  });
  if (result.status !== 0 || !result.stdout.trim()) return null;
  const match = result.stdout.trim().match(/^(\d+)\s+(.+?)\s+(\S+)$/u);
  if (!match || Number(match[1]) !== pid) return null;
  const start = match[2]!.trim().replace(/\s+/gu, " ");
  return /^(?:Sun|Mon|Tue|Wed|Thu|Fri|Sat) (?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec) \d{1,2} \d{2}:\d{2}:\d{2} \d{4}$/u.test(start) ? start : null;
}

export function processIsAlive(pid: number, startIdentity: string): boolean {
  return processStartIdentity(pid) === startIdentity;
}

export function processDefinitelyExited(pid: number, startIdentity: string): boolean {
  const probe = spawnSync("/bin/ps", ["-p", String(pid), "-o", "pid="], {
    encoding: "utf8", timeout: 5_000, env: { ...process.env, LC_ALL: "C" },
  });
  if (probe.error) return false;
  if (probe.status === 1 && !probe.stdout.trim()) return true;
  const current = processStartIdentity(pid);
  return Boolean(current && current !== startIdentity);
}

export function sourceCodexHome(explicit?: string): string {
  const requested = explicit ?? process.env.CODEX_HOME ?? join(homedir(), ".codex");
  if (!isAbsolute(requested)) throw new Error("source CODEX_HOME must be absolute");
  const requestedStat = lstatSync(requested);
  if (requestedStat.isSymbolicLink() || !requestedStat.isDirectory() ||
      (typeof process.getuid === "function" && requestedStat.uid !== process.getuid()) ||
      (requestedStat.mode & 0o022) !== 0) throw new Error("source CODEX_HOME must be a private user directory");
  const real = realpathSync(requested);
  const stat = lstatSync(real);
  if (stat.isSymbolicLink() || !stat.isDirectory() ||
      (typeof process.getuid === "function" && stat.uid !== process.getuid()) || (stat.mode & 0o022) !== 0) {
    throw new Error("source CODEX_HOME must be a private real directory");
  }
  return real;
}

export function requireNoExistingPrivateSession(): void {
  try {
    const stat = lstatSync(SESSIONS_ROOT);
    if (stat.isSymbolicLink() || !stat.isDirectory()) throw new Error("session store is not a real directory");
    const active = readdirSync(SESSIONS_ROOT, { withFileTypes: true }).filter((entry) => entry.isDirectory() && SESSION_ID.test(entry.name));
    if (active.length) throw new Error("existing private sessions could be swept by the production open path");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }
  const targetStat = lstatSync(TARGET_STATE);
  if (targetStat.isSymbolicLink() || !targetStat.isDirectory()) throw new Error("target owner state is not a real directory");
  const ownerNames = readdirSync(TARGET_STATE, { withFileTypes: true })
    .filter((entry) => entry.name === "incognito.lock" || entry.name.startsWith("incognito.lock.active.") || entry.name.startsWith(".incognito.lock.quarantine."));
  if (ownerNames.length) throw new Error("an existing Incodex child-owner record blocks fresh-window acceptance");
}

export function copyAllowlistedCodexFiles(source: string, destination: string): string[] {
  mkdirSync(destination, { recursive: true, mode: 0o700 });
  chmodSync(destination, 0o700);
  const copied: string[] = [];
  for (const name of ["auth.json", "config.toml"]) {
    const from = join(source, name);
    try {
      const stat = lstatSync(from);
      if (stat.isSymbolicLink() || !stat.isFile() || (stat.mode & 0o022) !== 0 || stat.size > 4 * 1024 * 1024 ||
          (typeof process.getuid === "function" && stat.uid !== process.getuid())) {
        throw new Error("source allowlisted Codex file is not a safe regular file");
      }
      const fd = openSync(from, fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW || 0));
      let bytes: Buffer;
      try {
        const opened = fstatSync(fd);
        if (!opened.isFile() || opened.dev !== stat.dev || opened.ino !== stat.ino || opened.size > 4 * 1024 * 1024) {
          throw new Error("source allowlisted Codex file changed during the bounded read");
        }
        bytes = readFileSync(fd);
      } finally { closeSync(fd); }
      const to = join(destination, name);
      writeFileSync(to, bytes, { mode: 0o600, flag: "wx" });
      chmodSync(to, 0o600);
      copied.push(name);
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    }
  }
  if (!copied.includes("auth.json")) throw new Error("isolated CODEX_HOME needs the allowlisted auth.json file");
  const extra = readdirSync(destination).filter((name) => !["auth.json", "config.toml"].includes(name));
  if (extra.length) throw new Error("isolated CODEX_HOME contains a non-allowlisted file");
  return copied;
}

export function currentSessionIds(): Set<string> {
  try {
    const stat = lstatSync(SESSIONS_ROOT);
    if (stat.isSymbolicLink() || !stat.isDirectory()) throw new Error("session root is not a real directory");
    return new Set(readdirSync(SESSIONS_ROOT, { withFileTypes: true })
      .filter((entry) => entry.isDirectory() && SESSION_ID.test(entry.name)).map((entry) => entry.name));
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return new Set();
    throw error;
  }
}

export function safeReadSessionOwner(sessionId: string): { pid: number; processStartIdentity: string } {
  const rootStat = lstatSync(SESSIONS_ROOT);
  if (rootStat.isSymbolicLink() || !rootStat.isDirectory()) throw new Error("private session root changed");
  const sessionRoot = join(SESSIONS_ROOT, sessionId);
  const sessionStat = lstatSync(sessionRoot);
  if (sessionStat.isSymbolicLink() || !sessionStat.isDirectory()) throw new Error("new private session root changed");
  const ownerFile = join(sessionRoot, "owner.json");
  const ownerStat = lstatSync(ownerFile);
  if (ownerStat.isSymbolicLink() || !ownerStat.isFile() || ownerStat.size > 8 * 1024) throw new Error("new private session owner evidence is invalid");
  const fd = openSync(ownerFile, fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW || 0));
  try {
    const opened = fstatSync(fd);
    if (!opened.isFile() || opened.dev !== ownerStat.dev || opened.ino !== ownerStat.ino || opened.size > 8 * 1024) {
      throw new Error("new private session owner changed during the bounded read");
    }
    const owner = JSON.parse(readFileSync(fd, "utf8"));
    if (!isRecord(owner) || owner.sessionId !== sessionId || !Number.isSafeInteger(owner.pid) ||
        typeof owner.processStartIdentity !== "string" || !owner.processStartIdentity) {
      throw new Error("new private session has no exact owner binding");
    }
    if (processStartIdentity(Number(owner.pid)) !== owner.processStartIdentity || !processUsesOfficialBinary(Number(owner.pid))) {
      throw new Error("new private session owner is not the exact live official child");
    }
    return { pid: Number(owner.pid), processStartIdentity: owner.processStartIdentity };
  } finally { closeSync(fd); }
}

function processUsesOfficialBinary(pid: number): boolean {
  const result = spawnSync("/bin/ps", ["-p", String(pid), "-o", "command="], {
    encoding: "utf8", timeout: 5_000, env: { ...process.env, LC_ALL: "C" },
  });
  if (result.status !== 0) return false;
  const command = result.stdout.trim();
  return command === PRODUCT_BINARY || command.startsWith(`${PRODUCT_BINARY} `);
}

export async function waitForNewSession(before: Set<string>): Promise<{ id: string; owner: { pid: number; processStartIdentity: string } }> {
  const deadline = Date.now() + CHILD_TIMEOUT_MS;
  while (Date.now() < deadline) {
    const after = currentSessionIds();
    const added = [...after].filter((id) => !before.has(id));
    if (added.length > 1) throw new Error("A open created more than one private session");
    if (added.length === 1) {
      try { return { id: added[0]!, owner: safeReadSessionOwner(added[0]!) }; }
      catch { /* owner handoff is an asynchronous part of this one production launch */ }
    }
    await delay(POLL_MS);
  }
  throw new Error("A open did not create one verifiable private session");
}

function sessionProcessIds(sessionId: string): number[] {
  const sessionRoot = join(SESSIONS_ROOT, sessionId);
  const output = runReadOnly("/bin/ps", ["axEww", "-o", "pid=,command="], 10_000);
  const marker = `INCODEX_SESSION_ROOT=${sessionRoot}`;
  return output.split(/\r?\n/u).flatMap((line) => {
    const match = line.match(/^\s*(\d+)\s+(.+)$/u);
    if (!match) return [];
    const found = match[2]!.includes(marker);
    return found ? [Number(match[1])] : [];
  });
}

export async function waitForSessionBurn(sessionId: string, owner: { pid: number; processStartIdentity: string }): Promise<void> {
  const deadline = Date.now() + CLOSE_TIMEOUT_MS;
  while (Date.now() < deadline) {
    const stillExists = currentSessionIds().has(sessionId);
    const ownerAlive = processIsAlive(owner.pid, owner.processStartIdentity);
    const sessionProcesses = sessionProcessIds(sessionId);
    if (!stillExists && !ownerAlive && sessionProcesses.length === 0) return;
    await delay(POLL_MS);
  }
  throw new Error("the exact private window did not close and burn normally; no process was terminated");
}

function checkAxFrontmost(helper: string): void {
  const raw = runReadOnly(helper, ["frontmost"], 10_000);
  const result = JSON.parse(raw);
  if (result.ok !== true || result.securityAgentActive === true || result.systemSettingsActive === true ||
      result.securityAgentWindowVisible === true || result.systemSettingsWindowVisible === true) {
    throw new Error("a system permission/settings surface blocks safe AX close");
  }
}

export function closePrivateWindow(helper: string, owner: { pid: number; processStartIdentity: string }): number {
  if (!processIsAlive(owner.pid, owner.processStartIdentity)) throw new Error("exact private window owner exited before AX close");
  checkAxFrontmost(helper);
  runReadOnly(helper, ["activate", String(owner.pid), PRODUCT_BINARY], 15_000);
  const inspect = JSON.parse(runReadOnly(helper, ["inspect", String(owner.pid), PRODUCT_BINARY,
    JSON.stringify({ open: ["__unused__"], exit: ["__unused__"], search: ["__unused__"] })], 15_000));
  if (inspect.ok !== true || !Array.isArray(inspect.windows)) throw new Error("exact private window AX identity is unavailable");
  const main = inspect.windows.filter((row: any) => row?.isMain === true && Number.isSafeInteger(row.windowId) && row.windowId > 0);
  if (main.length !== 1) throw new Error("private window does not expose one unique native main window");
  const windowId = main[0]!.windowId;
  checkAxFrontmost(helper);
  const closed = JSON.parse(runReadOnly(helper, ["close-window", String(owner.pid), PRODUCT_BINARY, String(windowId)], 15_000));
  if (closed.ok !== true || closed.performed !== true || closed.windowId !== windowId) {
    throw new Error("exact private window native close was not acknowledged");
  }
  return windowId;
}

export function readAckEvents(): Array<RuntimeAck & { _line: number }> {
  let stat: ReturnType<typeof lstatSync>;
  try { stat = lstatSync(ACK_LOG); }
  catch (error) { if ((error as NodeJS.ErrnoException).code === "ENOENT") return []; throw new Error("Runtime ACK log cannot be inspected safely"); }
  if (stat.isSymbolicLink() || !stat.isFile() || stat.size > MAX_LOG_BYTES) throw new Error("Runtime ACK log is not a bounded regular file");
  const fd = openSync(ACK_LOG, fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW || 0));
  let raw: string;
  try {
    const opened = fstatSync(fd);
    if (!opened.isFile() || opened.dev !== stat.dev || opened.ino !== stat.ino || opened.size > MAX_LOG_BYTES) {
      throw new Error("Runtime ACK log changed during the bounded read");
    }
    raw = readFileSync(fd, "utf8");
  } finally { closeSync(fd); }
  const lines = raw.split(/\r?\n/u);
  const rows: Array<RuntimeAck & { _line: number }> = [];
  for (const [lineNumber, line] of lines.entries()) {
    const marker = " renderer-runtime-update ";
    const index = line.indexOf(marker);
    if (index < 0) continue;
    try {
      const event = JSON.parse(line.slice(index + marker.length));
      if (isRecord(event) && event.schemaVersion === 1) rows.push({ ...event, _line: lineNumber } as RuntimeAck & { _line: number });
    } catch { /* ignore non-JSON or truncated unrelated records */ }
  }
  return rows;
}

export function acceptedAckCount(expected: AckExpectation): number {
  return readAckEvents().filter((row) => acceptedRuntimeAck(row, expected)).length;
}

export async function waitForAck(expected: AckExpectation, previousCount = 0): Promise<RuntimeAck> {
  const deadline = Date.now() + ACK_TIMEOUT_MS;
  while (Date.now() < deadline) {
    const matches = readAckEvents().filter((row) => acceptedRuntimeAck(row, expected));
    if (matches.length > previousCount) return matches.at(-1)!;
    await delay(POLL_MS);
  }
  throw new Error("the expected active Runtime ACK was not recorded for this process and window");
}

export async function waitForPort(pid: number, port: number): Promise<void> {
  const deadline = Date.now() + STARTUP_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (!processStartIdentity(pid)) throw new Error("isolated host exited before CDP readiness");
    const lsof = spawnSync("/usr/sbin/lsof", ["-nP", `-iTCP:${port}`, "-sTCP:LISTEN", "-t"], {
      encoding: "utf8", timeout: 5_000, env: { ...process.env, LC_ALL: "C" },
    });
    const owners = (lsof.status === 0 ? lsof.stdout : "").trim().split(/\s+/u).filter(Boolean).map(Number);
    if (owners.length === 1 && owners[0] === pid) {
      try {
        const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(2_000) });
        if (response.ok) return;
      } catch { /* keep waiting for the exact loopback-owned listener */ }
    } else if (owners.length > 1 || (owners.length === 1 && owners[0] !== pid)) {
      throw new Error("CDP port is not owned exclusively by the isolated host PID");
    }
    await delay(POLL_MS);
  }
  throw new Error("isolated host did not expose its exact loopback CDP listener");
}

export type CdpTarget = { id: string; type: string; url: string; webSocketDebuggerUrl: string };

export async function pageTargets(port: number): Promise<CdpTarget[]> {
  const response = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(5_000) });
  if (!response.ok) throw new Error("CDP target list is unavailable");
  const targets = await response.json();
  if (!Array.isArray(targets)) throw new Error("CDP target list is invalid");
  return targets.filter((row) => row?.type === "page" && typeof row.id === "string" &&
    typeof row.url === "string" && typeof row.webSocketDebuggerUrl === "string");
}

export class CdpClient {
  private nextId = 1;
  private pending = new Map<number, { resolve: (value: any) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  private socket: WebSocket;

  private constructor(socket: WebSocket) { this.socket = socket; }

  static async connect(url: string): Promise<CdpClient> {
    const socket = new WebSocket(url);
    await new Promise<void>((resolveOpen, reject) => {
      const timeout = setTimeout(() => reject(new Error("CDP WebSocket open timed out")), 10_000);
      socket.addEventListener("open", () => { clearTimeout(timeout); resolveOpen(); }, { once: true });
      socket.addEventListener("error", () => { clearTimeout(timeout); reject(new Error("CDP WebSocket failed")); }, { once: true });
    });
    const client = new CdpClient(socket);
    socket.addEventListener("message", (event) => client.receive(String(event.data)));
    socket.addEventListener("close", () => client.failPending());
    socket.addEventListener("error", () => client.failPending());
    return client;
  }

  private receive(raw: string): void {
    let message: any;
    try { message = JSON.parse(raw); } catch { return; }
    if (!Number.isSafeInteger(message.id)) return;
    const pending = this.pending.get(message.id);
    if (!pending) return;
    clearTimeout(pending.timer);
    this.pending.delete(message.id);
    if (message.error) pending.reject(new Error("CDP command returned an error"));
    else pending.resolve(message.result);
  }

  private failPending(): void {
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(new Error("CDP connection closed"));
    }
    this.pending.clear();
  }

  send(method: string, params: Record<string, unknown> = {}, timeoutMs = 10_000): Promise<any> {
    if (this.socket.readyState !== WebSocket.OPEN) return Promise.reject(new Error("CDP connection is not open"));
    const id = this.nextId++;
    return new Promise((resolveResult, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error("CDP command timed out"));
      }, timeoutMs);
      this.pending.set(id, { resolve: resolveResult, reject, timer });
      this.socket.send(JSON.stringify({ id, method, params }));
    });
  }

  async evaluate<T>(expression: string): Promise<T> {
    const reply = await this.send("Runtime.evaluate", {
      expression, awaitPromise: true, returnByValue: true, userGesture: true,
    }, 30_000);
    if (reply?.exceptionDetails || !reply?.result || !Object.hasOwn(reply.result, "value")) {
      throw new Error("isolated renderer evaluation was not acknowledged");
    }
    return reply.result.value as T;
  }

  close(): void {
    if (this.socket.readyState === WebSocket.OPEN) this.socket.close();
    this.failPending();
  }
}

export async function waitForMainPage(port: number): Promise<{ target: CdpTarget; cdp: CdpClient }> {
  const deadline = Date.now() + STARTUP_TIMEOUT_MS;
  while (Date.now() < deadline) {
    const pages = await pageTargets(port);
    const candidates: Array<{ target: CdpTarget; cdp: CdpClient }> = [];
    for (const target of pages) {
      if (!/^(?:file:|app:|https:\/\/chatgpt\.com(?:\/|$))/u.test(target.url)) continue;
      let cdp: CdpClient | null = null;
      try {
        cdp = await CdpClient.connect(target.webSocketDebuggerUrl);
        const api = await cdp.evaluate<boolean>("Boolean(window.incodex && typeof window.incodex.requestIncognitoAction === 'function' && window.__incodexIncognito === false)");
        if (api) candidates.push({ target, cdp }); else cdp.close();
      } catch { cdp?.close(); }
    }
    if (candidates.length === 1) return candidates[0]!;
    for (const candidate of candidates) candidate.cdp.close();
    if (candidates.length > 1) throw new Error("isolated host exposed multiple eligible main renderer pages");
    await delay(POLL_MS);
  }
  throw new Error("normal isolated host did not expose its production action IPC");
}

export async function reservePort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((resolveListen, reject) => {
    server.once("error", reject);
    server.listen({ host: "127.0.0.1", port: 0 }, () => resolveListen());
  });
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("could not reserve a loopback port");
  const port = address.port;
  await new Promise<void>((resolveClose, reject) => server.close((error) => error ? reject(error) : resolveClose()));
  return port;
}

export function isolatedHostEnvironment(codexHome: string, userData: string): NodeJS.ProcessEnv {
  const env = { ...process.env };
  for (const name of Object.keys(env)) if (name.startsWith("INCODEX_")) delete env[name];
  env.HOME = homedir();
  env.CODEX_HOME = codexHome;
  env.CODEX_ELECTRON_USER_DATA_PATH = userData;
  return env;
}

export async function sentinelCheck(cdp: CdpClient, sentinel: string, initialize: boolean): Promise<boolean> {
  const encoded = JSON.stringify(sentinel);
  const expression = initialize
    ? `(() => { if (!window.incodex || window.__incodexIncognito !== false) return false; Object.defineProperty(window, '__incodexHotAcceptanceSentinel', { value: ${encoded}, configurable: false, writable: false }); return window.__incodexHotAcceptanceSentinel === ${encoded}; })()`
    : `window.__incodexHotAcceptanceSentinel === ${encoded} && window.__incodexIncognito === false`;
  return cdp.evaluate<boolean>(expression);
}

export async function invokeProductionOpen(cdp: CdpClient, expected: ActionExpectation): Promise<void> {
  const requestId = randomUUID();
  const result = await cdp.evaluate<unknown>(
    `window.incodex.requestIncognitoAction({ action: "open", requestId: ${JSON.stringify(requestId)} })`,
  );
  verifyActionResult(result, expected);
}

export async function waitForCurrent(evidence: CandidateEvidence): Promise<void> {
  const deadline = Date.now() + ACK_TIMEOUT_MS;
  while (Date.now() < deadline) {
    try { requireCurrentCandidate(readCurrentPointer(), evidence, "published Runtime"); return; }
    catch { await delay(POLL_MS); }
  }
  throw new Error("CLI did not publish the exact frozen Runtime generation");
}

export function writeSafeReport(path: string, report: Report): void {
  const temporary = `${path}.tmp`;
  writeFileSync(temporary, `${JSON.stringify(report, null, 2)}\n`, { mode: 0o600, flag: "wx" });
  chmodSync(temporary, 0o600);
  renameSync(temporary, path);
}

export function addEvent(report: Report, event: Record<string, string | number | boolean | null>): void {
  report.events.push(event);
}

export function runAxCompile(output: string): string {
  if (process.platform !== "darwin") throw new Error("the explicit AX acceptance is macOS-only");
  const swiftSource = lstatSync(AX_SOURCE);
  if (swiftSource.isSymbolicLink() || !swiftSource.isFile()) throw new Error("exact AX helper source is unavailable");
  const binary = join(output, "incodex-acceptance-ax");
  const result = spawnSync("/usr/bin/swiftc", ["-O", AX_SOURCE, "-o", binary], {
    encoding: "utf8", timeout: 120_000, env: { ...process.env, LC_ALL: "C" },
  });
  if (result.error || result.status !== 0) throw new Error("existing exact-PID AX helper failed to compile");
  chmodSync(binary, 0o700);
  return binary;
}

export function checkAppAndRuntimePreflight(aCli: string, bCli: string, a: CandidateEvidence, b: CandidateEvidence): void {
  if (process.platform !== "darwin") throw new Error("the explicit Runtime hot acceptance is macOS-only");
  const appStat = lstatSync(PRODUCT_APP);
  const binaryStat = lstatSync(PRODUCT_BINARY);
  if (appStat.isSymbolicLink() || !appStat.isDirectory() || binaryStat.isSymbolicLink() || !binaryStat.isFile()) {
    throw new Error("fixed official ChatGPT bundle or executable is unavailable");
  }
  requireNoOfficialHost();
  verifyCliVersion(runReadOnly(aCli, ["--version"]), a.cliVersion, "A");
  verifyCliVersion(runReadOnly(bCli, ["--version"]), b.cliVersion, "B");
  const aStatus = readCandidateStatus(aCli, "A");
  const bStatus = readCandidateStatus(bCli, "B");
  requireBundledIdentity(aStatus, a, "A");
  requireBundledIdentity(bStatus, b, "B");
  if (aStatus.externalRuntime?.present !== true || aStatus.externalRuntime.ok !== true ||
      aStatus.externalRuntime.state !== "current" || aStatus.externalRuntime.manifestSha256 !== a.runtimeManifestSha256 ||
      bStatus.externalRuntime?.present !== true || bStatus.externalRuntime.ok !== true ||
      bStatus.externalRuntime.manifestSha256 !== a.runtimeManifestSha256) {
    throw new Error("A must be the current verified Runtime before the isolated host starts");
  }
  const current = readCurrentPointer();
  requireCurrentCandidate(current, a, "A");
  requireNoExistingPrivateSession();
}

export function delay(ms: number): Promise<void> { return new Promise((resolveDelay) => setTimeout(resolveDelay, ms)); }
