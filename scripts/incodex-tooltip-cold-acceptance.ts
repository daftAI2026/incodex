#!/usr/bin/env bun
import { createHash } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";
import { SEARCH_LABELS } from "../src/runtime/compatibility/search-labels.ts";

type Mode = "run" | "self-test";
type Options = {
  mode: Mode;
  cli?: string;
  parentPid?: number;
  output?: string;
  toleranceMs: number;
};
type ProcessRow = { pid: number; ppid: number; started: string; command: string };
type DebuggerChild = ProcessRow & { port: number };
type CdpTarget = { id: string; type: string; url: string; webSocketDebuggerUrl: string };
type CdpReply = { id?: number; result?: unknown; error?: { message?: string } };
type HitTestNode = {
  tag: string;
  role: string | null;
  testId: string | null;
  dataState: string | null;
  ariaModal: boolean;
  idHash: string | null;
  classHash: string | null;
  position: string;
  zIndex: string;
  pointerEvents: string;
  bounds: { x: number; y: number; width: number; height: number };
};
type HitTestDiagnostic = {
  hit: boolean;
  point: { x: number; y: number } | null;
  stack: HitTestNode[];
};
type TooltipState = {
  incognito: boolean;
  button: { x: number; y: number; width: number; height: number } | null;
  search: { x: number; y: number; width: number; height: number } | null;
  viewport: { width: number; height: number };
  nowMs: number;
  documentFocused: boolean;
  hatHitTest: boolean;
  searchHitTest: boolean;
  hatHitTarget: HitTestDiagnostic;
  searchHitTarget: HitTestDiagnostic;
  hatHovered: boolean;
  hatTooltipOpen: boolean;
  hatTooltipClass: string | null;
  searchTooltipOpen: boolean;
  searchTooltipClass: string | null;
  titlePresent: boolean;
  legacyTooltipOpen: boolean;
  legacyTooltipVisible: boolean;
  tooltipRootCount: number;
  rendererReady: boolean;
  lifecyclePresent: boolean;
  searchTooltipSampleAvailable: boolean;
  indexScriptSrc: string | null;
  tooltipCandidates: Array<{
    name: string | null;
    sourceLength: number;
    sourceFeatures: string[];
    timingProps: Record<string, number | boolean>;
  }>;
  provider: {
    found: boolean;
    delayMs: number | null;
    hoverOpenBlocked: boolean | null;
    timingProps: Record<string, number | boolean>;
  };
};
type ListenerRow = { pid: number; endpoint: string };
type ProbeEvent = { sequence: number; type: string; target: string; trusted: boolean; timeMs: number };

const root = resolve(import.meta.dir, "..");
const bundleIdentifier = "com.openai.codex";
const pollIntervalMs = 40;
const startupTimeoutMs = 90_000;
const tooltipTimeoutMs = 3_000;
const closeTimeoutMs = 30_000;
const coldIdleMs = 900;
const statusFields = [
  "target",
  "exists",
  "patched",
  "bundleId",
  "appVersion",
  "appBuild",
  "architecture",
  "asarFileHash",
  "plistFileHash",
  "asarLoaderOnly",
] as const;

function usage(): string {
  return [
    "Installed candidate cold-tooltip acceptance (macOS only).",
    "Requires the exact foreground-independent ChatGPT parent PID and a CLI built with the candidate Runtime.",
    "  bun scripts/incodex-tooltip-cold-acceptance.ts --self-test",
    "  bun scripts/incodex-tooltip-cold-acceptance.ts --run --cli /absolute/path/to/incodex --parent-pid PID --out /private/tmp/new-tooltip-evidence",
    "The run reads only install/process metadata and the Search/hat tooltip DOM. It does not read chat/profile content, capture screenshots, change TCC, install, publish, or delete session files.",
  ].join("\n");
}

export function parseArguments(args: string[]): Options {
  let mode: Mode | null = null;
  let cli: string | undefined;
  let parentPid: number | undefined;
  let output: string | undefined;
  let toleranceMs = 300;
  const seen = new Set<string>();

  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index]!;
    if (arg === "--run" || arg === "--self-test") {
      if (mode) throw new Error("choose exactly one of --run or --self-test");
      mode = arg === "--run" ? "run" : "self-test";
      continue;
    }
    if (arg === "--cli" || arg === "--parent-pid" || arg === "--out" || arg === "--tolerance-ms") {
      if (seen.has(arg)) throw new Error(`${arg} may be provided only once`);
      seen.add(arg);
      const value = args[index + 1];
      if (!value || value.startsWith("--")) throw new Error(`${arg} requires a value`);
      index += 1;
      if (arg === "--cli") cli = value;
      else if (arg === "--parent-pid") {
        const parsed = Number(value);
        if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error("--parent-pid requires a positive PID");
        parentPid = parsed;
      } else if (arg === "--out") output = value;
      else {
        toleranceMs = Number(value);
        if (!Number.isSafeInteger(toleranceMs) || toleranceMs < 0 || toleranceMs > 2_000) {
          throw new Error("--tolerance-ms must be an integer from 0 to 2000");
        }
      }
      continue;
    }
    if (arg === "--help" || arg === "-h") throw new Error(usage());
    throw new Error(`unknown argument: ${arg}`);
  }

  if (!mode) throw new Error(usage());
  if (mode === "self-test") {
    if (cli || parentPid || output || seen.size) throw new Error("--self-test does not accept run options");
    return { mode, toleranceMs };
  }
  if (!cli || !isAbsolute(cli)) throw new Error("--run requires --cli with an absolute executable path");
  if (!parentPid) throw new Error("--run requires --parent-pid with the exact running ChatGPT PID");
  if (!output || !isAbsolute(output)) throw new Error("--run requires --out with a new absolute output directory");
  return { mode, cli, parentPid, output, toleranceMs };
}

export function parseDryRunTargets(output: string): { app: string; binary: string } {
  const app = output.match(/^\s*App\s+(.+?)\s*$/mu)?.[1];
  const binary = output.match(/^\s*Binary\s+(.+?)\s*$/mu)?.[1];
  if (!app || !binary || !output.includes("Dry run. No window opened.")) {
    throw new Error("incodex open --dry-run did not identify a safe target");
  }
  return { app: resolve(app), binary: resolve(binary) };
}

export function parseProcessTable(output: string): ProcessRow[] {
  const rows: ProcessRow[] = [];
  for (const line of output.split(/\r?\n/u)) {
    const fields = line.trim().split(/\s+/u);
    if (fields.length < 8) continue;
    const pid = Number(fields[0]);
    const ppid = Number(fields[1]);
    if (!Number.isSafeInteger(pid) || !Number.isSafeInteger(ppid)) continue;
    rows.push({ pid, ppid, started: fields.slice(2, 7).join(" "), command: fields.slice(7).join(" ") });
  }
  return rows;
}

export function executableCommand(row: ProcessRow, binary: string): boolean {
  return row.command === binary || row.command.startsWith(`${binary} `);
}

export function findDebuggerChild(rows: ProcessRow[], cliPid: number, binary: string): DebuggerChild {
  const candidates = rows.flatMap((row) => {
    if (row.ppid !== cliPid || !executableCommand(row, binary)) return [];
    const port = row.command.match(/(?:^|\s)--remote-debugging-port=(\d+)(?:\s|$)/u)?.[1];
    if (!port || Number(port) <= 0 || Number(port) > 65_535) return [];
    return [{ ...row, port: Number(port) }];
  });
  if (candidates.length !== 1) {
    throw new Error(`expected one exact ChatGPT CDP child of CLI PID ${cliPid}; found ${candidates.length}`);
  }
  return candidates[0]!;
}

export function findDebuggerChildIfStarted(rows: ProcessRow[], cliPid: number, binary: string): DebuggerChild | null {
  const candidates = rows.filter((row) => row.ppid === cliPid && executableCommand(row, binary));
  if (candidates.length === 0) return null;
  return findDebuggerChild(rows, cliPid, binary);
}

export function parseListenerRows(output: string): ListenerRow[] {
  return output.split(/\r?\n/u).flatMap((line) => {
    const fields = line.trim().split(/\s+/u);
    const pid = Number(fields[1]);
    const endpoint = line.match(/\bTCP\s+(\S+)\s+\(LISTEN\)$/u)?.[1];
    if (!endpoint || !Number.isSafeInteger(pid)) return [];
    return [{ pid, endpoint }];
  });
}

export function assertLoopbackListenerOwnership(
  rows: ListenerRow[],
  pid: number,
  port: number,
  authorizedPids: ReadonlySet<number> = new Set([pid]),
): void {
  if (rows.length === 0) throw new Error(`no CDP listener is bound to port ${port}`);
  if (!authorizedPids.has(pid)) throw new Error(`exact child PID ${pid} is not in the verified process tree`);
  const allowed = new Set([`127.0.0.1:${port}`, `[::1]:${port}`]);
  const owners = [...new Set(rows.map((row) => row.pid))];
  if (owners.some((owner) => !authorizedPids.has(owner))) {
    throw new Error(`CDP port ${port} is not owned exclusively by the verified child process tree`);
  }
  if (rows.some((row) => !allowed.has(row.endpoint))) {
    throw new Error(`CDP port ${port} is not bound exclusively to loopback`);
  }
}

export function verifiedDescendantPids(rows: ProcessRow[], rootPid: number): Set<number> {
  const descendants = new Set([rootPid]);
  let changed = true;
  while (changed) {
    changed = false;
    for (const row of rows) {
      if (!descendants.has(row.pid) && descendants.has(row.ppid)) {
        descendants.add(row.pid);
        changed = true;
      }
    }
  }
  return descendants;
}

export function validateWebSocketUrl(value: string, port: number): URL {
  const url = new URL(value);
  if (url.protocol !== "ws:" || !["127.0.0.1", "[::1]", "localhost"].includes(url.hostname) || Number(url.port) !== port) {
    throw new Error("CDP WebSocket is not on the verified loopback port");
  }
  return url;
}

export function assertRuntimeCandidate(status: Record<string, unknown>, expectedApp: string): void {
  const runtime = status.externalRuntime as Record<string, unknown> | undefined;
  if (
    status.target !== expectedApp ||
    status.exists !== true ||
    status.patched !== true ||
    status.bundleId !== bundleIdentifier ||
    status.asarLoaderOnly !== true ||
    typeof status.asarFileHash !== "string" ||
    typeof status.plistFileHash !== "string" ||
    runtime?.ok !== true ||
    runtime.matchesBundled !== true ||
    runtime.state !== "current"
  ) {
    throw new Error("target is not a patched official app with the CLI-matched current Runtime; no UI was touched");
  }
}

export function coldLatencyFailure(hatMs: number, searchMs: number, toleranceMs: number): string | null {
  if (hatMs < 0 || searchMs < 0 || toleranceMs < 0) return "latency measurements must be non-negative";
  if (hatMs - searchMs > toleranceMs) {
    return `cold hat was ${hatMs - searchMs}ms slower than official Search (tolerance ${toleranceMs}ms)`;
  }
  return null;
}

export function maySendEscapeToDismissAcceptanceTooltip(
  inputConfirmed: boolean,
  testOwnedTooltipVisible: boolean,
  documentFocused: boolean,
): boolean {
  return inputConfirmed && testOwnedTooltipVisible && documentFocused;
}

function runReadOnly(binary: string, args: string[], timeout = 30_000): string {
  const result = spawnSync(binary, args, {
    cwd: root,
    encoding: "utf8",
    timeout,
    env: { ...process.env, NO_COLOR: "1" },
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${basename(binary)} ${args.join(" ")} failed: ${(result.stderr || result.stdout).trim()}`);
  return result.stdout;
}

function processRows(): ProcessRow[] {
  return parseProcessTable(runReadOnly("ps", ["-axo", "pid=,ppid=,lstart=,command="]));
}

function processAlive(pid: number): boolean {
  return processRows().some((row) => row.pid === pid);
}

function sha256(path: string): string {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function hashAppFiles(app: string): { asar: string; infoPlist: string } {
  return {
    asar: sha256(join(app, "Contents/Resources/app.asar")),
    infoPlist: sha256(join(app, "Contents/Info.plist")),
  };
}

function lsofRows(port: number): ListenerRow[] {
  const result = spawnSync("lsof", ["-nP", `-iTCP:${port}`, "-sTCP:LISTEN"], { encoding: "utf8", timeout: 10_000 });
  if (result.error) throw result.error;
  if (result.status !== 0 && result.status !== 1) throw new Error(`lsof failed: ${(result.stderr || result.stdout).trim()}`);
  return parseListenerRows(result.stdout || "");
}

function assertInstalledCandidate(cli: string, dryRun: string): {
  app: string;
  binary: string;
  status: Record<string, unknown>;
} {
  const { app, binary } = parseDryRunTargets(dryRun);
  const status = JSON.parse(runReadOnly(cli, ["status", "--json"])) as Record<string, unknown>;
  assertRuntimeCandidate(status, app);
  const expectedBinary = join(app, "Contents/MacOS/ChatGPT");
  if (binary !== expectedBinary || !existsSync(binary)) throw new Error("dry-run binary does not match the official target executable");
  return { app, binary, status };
}

function openOutputDirectory(path: string): string {
  const resolved = resolve(path);
  if (existsSync(resolved)) throw new Error("--out must be a new directory; existing paths are never overwritten");
  mkdirSync(resolved, { mode: 0o700 });
  return realpathSync(resolved);
}

function statusSummary(status: Record<string, unknown>): Record<string, unknown> {
  const summary: Record<string, unknown> = {};
  for (const field of statusFields) summary[field] = status[field] ?? null;
  const runtime = status.externalRuntime as Record<string, unknown>;
  summary.externalRuntime = {
    ok: runtime.ok,
    version: runtime.version,
    matchesBundled: runtime.matchesBundled,
    state: runtime.state,
    manifestSha256: runtime.manifestSha256,
  };
  return summary;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, ms));
}

async function waitFor<T>(
  label: string,
  read: () => Promise<T>,
  accept: (value: T) => boolean,
  timeoutMs: number,
  childState: { exited: boolean; exitCode: number | null },
): Promise<T> {
  const start = Date.now();
  while (Date.now() - start <= timeoutMs) {
    if (childState.exited) throw new Error(`CLI exited before ${label} (code ${childState.exitCode})`);
    const value = await read();
    if (accept(value)) return value;
    await delay(pollIntervalMs);
  }
  throw new Error(`timed out waiting for ${label} after ${timeoutMs}ms`);
}

async function observeUntil<T>(
  read: () => Promise<T>,
  accept: (value: T) => boolean,
  timeoutMs: number,
  childState: { exited: boolean; exitCode: number | null },
  startedAt: number,
): Promise<{ matched: boolean; elapsedMs: number; value: T }> {
  let value = await read();
  while (performance.now() - startedAt <= timeoutMs) {
    if (childState.exited) throw new Error(`CLI exited during tooltip measurement (code ${childState.exitCode})`);
    if (accept(value)) return { matched: true, elapsedMs: Math.round(performance.now() - startedAt), value };
    await delay(pollIntervalMs);
    value = await read();
  }
  return { matched: accept(value), elapsedMs: Math.round(performance.now() - startedAt), value };
}

type HitTestReadiness = Pick<TooltipState, "documentFocused" | "hatHitTest" | "searchHitTest">;
type StableHitTestResult<T> = {
  stable: boolean;
  waitMs: number;
  samples: number;
  state: T;
  firstBlocked: T | null;
};

export async function waitForStableHitTest<T extends HitTestReadiness>(
  read: () => Promise<T>,
  childState: { exited: boolean; exitCode: number | null },
  timeoutMs = 8_000,
  requiredSamples = 3,
  intervalMs = 100,
): Promise<StableHitTestResult<T>> {
  const started = performance.now();
  let state = await read();
  let firstBlocked: T | null = null;
  let samples = 0;
  let consecutive = 0;
  while (true) {
    if (childState.exited) throw new Error(`CLI exited during hit-test readiness wait (code ${childState.exitCode})`);
    samples += 1;
    if (state.documentFocused && state.hatHitTest && state.searchHitTest) {
      consecutive += 1;
      if (consecutive >= requiredSamples) {
        return { stable: true, waitMs: Math.round(performance.now() - started), samples, state, firstBlocked };
      }
    } else {
      consecutive = 0;
      firstBlocked ??= state;
    }
    if (performance.now() - started >= timeoutMs) {
      return { stable: false, waitMs: Math.round(performance.now() - started), samples, state, firstBlocked };
    }
    await delay(intervalMs);
    state = await read();
  }
}

export function tooltipStateExpression(): string {
  const labels = JSON.stringify([...SEARCH_LABELS]);
  return `(()=>{
    const b=document.querySelector('[data-incodex-privacy-toggle]');
    const s=[...document.querySelectorAll('button[aria-label]')].find(e=>${labels}.includes((e.getAttribute('aria-label')||'').trim()));
    const rect=e=>{if(!e)return null;const r=e.getBoundingClientRect();return{x:r.x,y:r.y,width:r.width,height:r.height}};
    const visible=e=>!!e&&e.isConnected&&e.getBoundingClientRect().width>0&&getComputedStyle(e).visibility!=='hidden'&&getComputedStyle(e).display!=='none';
    const token=v=>typeof v==='string'&&/^[A-Za-z0-9_.:-]{1,80}$/.test(v)?v:null;
    const hash=v=>{if(!v)return null;let h=2166136261;for(let i=0;i<v.length;i++){h^=v.charCodeAt(i);h=Math.imul(h,16777619)}return(h>>>0).toString(16).padStart(8,'0')};
    const nodeInfo=e=>{const r=e.getBoundingClientRect(),style=getComputedStyle(e);return{tag:e.tagName,role:token(e.getAttribute('role')),testId:token(e.getAttribute('data-testid')),dataState:token(e.getAttribute('data-state')),ariaModal:e.getAttribute('aria-modal')==='true',idHash:hash(e.id),classHash:hash(typeof e.className==='string'?e.className:String(e.className)),position:token(style.position)||'other',zIndex:token(style.zIndex)||'other',pointerEvents:token(style.pointerEvents)||'other',bounds:{x:Math.round(r.x),y:Math.round(r.y),width:Math.round(r.width),height:Math.round(r.height)}}};
    const hitInfo=e=>{if(!e?.isConnected)return{hit:false,point:null,stack:[]};const r=e.getBoundingClientRect(),point={x:Math.round(r.x+r.width/2),y:Math.round(r.y+r.height/2)},top=document.elementFromPoint(point.x,point.y);const stack=[];for(let n=top;n&&stack.length<4;n=n.parentElement)stack.push(nodeInfo(n));return{hit:!!top&&(top===e||e.contains(top)),point,stack}};
    const described=e=>[e,e?.parentElement].flatMap(x=>(x?.getAttribute('aria-describedby')||'').split(/\\s+/)).filter(Boolean).map(id=>document.getElementById(id)).find(t=>t?.getAttribute('role')==='tooltip');
    const hatTip=document.getElementById('incodex-official-tooltip');
    const searchTip=described(s);
    const legacyHost=document.querySelector('[data-incodex-tooltip-host]');
    const legacyTip=legacyHost?.querySelector('[data-incodex-tooltip]');
    const hatHitInfo=hitInfo(b),searchHitInfo=hitInfo(s);
    let provider=null;
    let fiber=(()=>{if(!s)return null;const k=Object.keys(s).find(k=>k.startsWith('__reactFiber$'));return k?s[k]:null})();
    for(let depth=0;fiber&&depth<64;depth++,fiber=fiber.return){
      let context=fiber.dependencies?.firstContext;
      for(let index=0;context&&index<64;index++,context=context.next){
        const p=context.memoizedValue;
        if(p&&typeof p.getOpenDelay==='function'&&typeof p.isHoverOpenBlocked==='function'&&typeof p.activateTooltip==='function'){provider=p;break}
      }
      if(provider)break;
    }
    const timingProps={};
    const tooltipCandidates=[];
    for(let f=(()=>{if(!s)return null;const k=Object.keys(s).find(k=>k.startsWith('__reactFiber$'));return k?s[k]:null})(),depth=0;f&&depth<64;f=f.return,depth++){
      for(const props of [f.memoizedProps,f.pendingProps]){
        if(!props||typeof props!=='object')continue;
        for(const key of ['delayDuration','skipDelayDuration','disableHoverableContent','disableHoverOpen']){
          const value=props[key];if(typeof value==='number'||typeof value==='boolean')timingProps[key]=value;
        }
      }
      const type=f.elementType||f.type;
      if(typeof type==='function'&&tooltipCandidates.length<8){
        const name=type.displayName||type.name||null;
        let source='';try{source=Function.prototype.toString.call(type)}catch{}
        const sample=source.slice(0,40000);
        const featureMap={tooltip:/tooltip/i.test(sample),delayDuration:/delayDuration/i.test(sample),skipDelayDuration:/skipDelayDuration/i.test(sample),disableHoverableContent:/disableHoverableContent/i.test(sample),disableHoverOpen:/disableHoverOpen/i.test(sample)};
        const props={};for(const key of ['delayDuration','skipDelayDuration','disableHoverableContent','disableHoverOpen']){const value=f.memoizedProps?.[key]??f.pendingProps?.[key];if(typeof value==='number'||typeof value==='boolean')props[key]=value}
        if(/tooltip/i.test(name||'')||Object.values(featureMap).some(Boolean)||Object.keys(props).length){
          tooltipCandidates.push({name,sourceLength:source.length,sourceFeatures:Object.keys(featureMap).filter(key=>featureMap[key]),timingProps:props});
        }
      }
    }
    let delayMs=null,hoverOpenBlocked=null;
    try{if(provider)delayMs=provider.getOpenDelay('default',700)}catch{}
    try{if(provider)hoverOpenBlocked=provider.isHoverOpenBlocked('incodex-privacy-toggle')}catch{}
    return{
      incognito:window.__incodexIncognito===true,
      button:rect(b),search:rect(s),viewport:{width:innerWidth,height:innerHeight},
      nowMs:performance.now(),documentFocused:document.hasFocus(),hatHitTest:hatHitInfo.hit,searchHitTest:searchHitInfo.hit,hatHitTarget:hatHitInfo,searchHitTarget:searchHitInfo,
      hatHovered:b?.getAttribute('data-incodex-hovered')==='true',
      hatTooltipOpen:visible(hatTip),hatTooltipClass:hatTip?.className||null,
      searchTooltipOpen:visible(searchTip),searchTooltipClass:searchTip?.className||null,
      titlePresent:b?.hasAttribute('title')??false,
      legacyTooltipOpen:legacyHost?.hasAttribute('data-open')??false,
      legacyTooltipVisible:visible(legacyTip)&&legacyHost?.hasAttribute('data-open')===true,
      tooltipRootCount:document.querySelectorAll('[data-incodex-official-tooltip-root]').length,
      rendererReady:window.__incodexTooltipState?.renderer?.ready?.()===true,
      lifecyclePresent:!!window.__incodexTooltipState?.lifecycle,
      searchTooltipSampleAvailable:!!searchTip&&!!searchTip.className?.trim(),
      indexScriptSrc:[...document.querySelectorAll('script[type="module"][src]')].map(e=>e.src).find(src=>/\\/assets\\/index-[A-Za-z0-9_-]+\\.js(?:$|\\?)/.test(src))||null,
      tooltipCandidates,
      provider:{found:!!provider,delayMs,hoverOpenBlocked,timingProps}
    };
  })()`;
}

function responseValue<T>(response: unknown): T {
  const record = response as { exceptionDetails?: unknown; result?: { value?: T } };
  if (record.exceptionDetails) throw new Error("renderer state read failed");
  return record.result?.value as T;
}

function nextMessage(ws: WebSocket, sequence: number, method: string, params: Record<string, unknown> = {}): Promise<unknown> {
  return new Promise((resolvePromise, reject) => {
    const id = sequence;
    const timer = setTimeout(() => {
      ws.removeEventListener("message", receive);
      reject(new Error(`CDP timeout: ${method}`));
    }, 8_000);
    const receive = (event: MessageEvent) => {
      let message: CdpReply;
      try {
        message = JSON.parse(String(event.data)) as CdpReply;
      } catch {
        return;
      }
      if (message.id !== id) return;
      clearTimeout(timer);
      ws.removeEventListener("message", receive);
      if (message.error) reject(new Error(`CDP ${method} failed: ${message.error.message ?? "unknown error"}`));
      else resolvePromise(message.result);
    };
    ws.addEventListener("message", receive);
    ws.send(JSON.stringify({ id, method, params }));
  });
}

async function connectWebSocket(value: string): Promise<WebSocket> {
  const ws = new WebSocket(value);
  await new Promise<void>((resolvePromise, reject) => {
    const timer = setTimeout(() => reject(new Error("CDP WebSocket connection timed out")), 8_000);
    ws.addEventListener("open", () => { clearTimeout(timer); resolvePromise(); }, { once: true });
    ws.addEventListener("error", () => { clearTimeout(timer); reject(new Error("CDP WebSocket connection failed")); }, { once: true });
  });
  return ws;
}

function cdpClient(ws: WebSocket) {
  let sequence = 0;
  return {
    request: <T>(method: string, params: Record<string, unknown> = {}) => nextMessage(ws, ++sequence, method, params) as Promise<T>,
    async evaluate<T>(expression: string): Promise<T> {
      const result = await this.request("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
      return responseValue<T>(result);
    },
  };
}

function loopbackUrl(port: number, path: string): string {
  return `http://127.0.0.1:${port}${path}`;
}

async function readTargets(port: number): Promise<CdpTarget[]> {
  const response = await fetch(loopbackUrl(port, "/json/list"), { signal: AbortSignal.timeout(2_000) });
  if (!response.ok) throw new Error(`CDP target discovery returned HTTP ${response.status}`);
  return await response.json() as CdpTarget[];
}

function uniqueMainPage(targets: CdpTarget[]): CdpTarget {
  const matches = targets.filter((target) => target.type === "page" && target.url === "app://-/index.html");
  if (matches.length !== 1) throw new Error(`expected one main app page target; found ${matches.length}`);
  return matches[0]!;
}

function classHash(value: string | null): string | null {
  return value ? createHash("sha256").update(value).digest("hex") : null;
}

export function parseRendererPrepareWarning(value: string): { errorCode: string; modulePaths: string[] } {
  const text = value.slice(0, 20_000);
  const knownErrors: Array<[RegExp, string]> = [
    [/official\s+react\s+module\s+is\s+unavailable/i, "OFFICIAL_REACT_MODULE_UNAVAILABLE"],
    [/official\s+client\s+module\s+is\s+unavailable/i, "OFFICIAL_CLIENT_MODULE_UNAVAILABLE"],
    [/official\s+tooltip\s+module\s+is\s+unavailable/i, "OFFICIAL_TOOLTIP_MODULE_UNAVAILABLE"],
    [/official\s+renderer\s+entry\s+is\s+unavailable/i, "OFFICIAL_RENDERER_ENTRY_UNAVAILABLE"],
    [/cannot\s+read\s+official\s+renderer\s+entry/i, "OFFICIAL_RENDERER_ENTRY_READ_FAILED"],
    [/unsupported\s+official\s+react\s+renderer/i, "UNSUPPORTED_OFFICIAL_REACT_RENDERER"],
    [/unsupported\s+official\s+tooltip\s+exports/i, "UNSUPPORTED_OFFICIAL_TOOLTIP_EXPORTS"],
  ];
  const errorCode = knownErrors.find(([pattern]) => pattern.test(text))?.[1]
    ?? text.match(/\b(TypeError|SyntaxError|ReferenceError|RangeError|AbortError)\b/u)?.[1].toUpperCase()
    ?? (text.match(/\bHTTP\s+(\d{3})\b/u)?.[1] ? `HTTP_${text.match(/\bHTTP\s+(\d{3})\b/u)![1]}` : "UNCLASSIFIED_RENDERER_PREPARE_ERROR");
  const modulePaths = [...text.matchAll(/(?:app|file):\/\/[^\s"'<>]*\/assets\/[A-Za-z0-9._/-]+\.js/gu)]
    .map((match) => match[0]!.replace(/[),.;]+$/u, ""));
  return { errorCode, modulePaths: [...new Set(modulePaths)].slice(0, 4) };
}

function stateSummary(state: TooltipState): Record<string, unknown> {
  const { hatTooltipClass, searchTooltipClass, ...safe } = state;
  return {
    ...safe,
    hatTooltipClassSha256: classHash(hatTooltipClass),
    searchTooltipClassSha256: classHash(searchTooltipClass),
  };
}

export function tooltipEventProbeExpression(): string {
  const labels = JSON.stringify([...SEARCH_LABELS]);
  return `(()=>{
    const events=[];window.__incodexTooltipAcceptanceEvents=events;window.__incodexTooltipAcceptanceSequence=0;
    const labels=${labels};
    for(const name of ['pointerenter','pointerleave','focus','blur','keydown'])window.addEventListener(name,e=>{
      let target='other';
      if(e.target===window)target='window';
      else if(e.target instanceof Element){
        if(e.target.closest('[data-incodex-privacy-toggle]'))target='hat';
        else {const b=e.target.closest('button[aria-label]');if(b&&labels.includes((b.getAttribute('aria-label')||'').trim()))target='search'}
      }
      if(target==='hat'||target==='search'||(name==='keydown'&&e.key==='Escape')||((name==='focus'||name==='blur')&&target==='window')){
        events.push({sequence:++window.__incodexTooltipAcceptanceSequence,type:name,target:name==='keydown'?'window':target,trusted:e.isTrusted,timeMs:Math.round(performance.now())});
        if(events.length>12)events.shift();
      }
    },true);
    return true;
  })()`;
}

function outsidePoint(state: TooltipState): { x: number; y: number } {
  if (!state.button || !state.search) throw new Error("hat/Search bounds are unavailable");
  const controls = [state.button, state.search];
  const candidates = [
    { x: Math.min(state.viewport.width - 4, Math.max(...controls.map((item) => item.x + item.width)) + 72), y: Math.min(state.viewport.height - 4, Math.max(...controls.map((item) => item.y + item.height)) + 88) },
    { x: Math.max(4, Math.floor(state.viewport.width * 0.82)), y: Math.max(4, Math.floor(state.viewport.height * 0.8)) },
    { x: Math.max(4, Math.floor(state.viewport.width * 0.72)), y: Math.max(4, Math.floor(state.viewport.height * 0.68)) },
  ];
  const usable = candidates.find((point) => point.x >= 1 && point.y >= 1 && point.x < state.viewport.width && point.y < state.viewport.height);
  if (!usable) throw new Error("cannot find a safe pointer-out location in the current viewport");
  return usable;
}

async function runAcceptance(options: Options): Promise<Record<string, unknown>> {
  if (!options.cli || !options.parentPid || !options.output) throw new Error("run options are incomplete");
  if (process.platform !== "darwin") throw new Error("this acceptance script is macOS-only");
  const cli = realpathSync(options.cli);
  const cliStats = statSync(cli);
  if (!cliStats.isFile() || (cliStats.mode & 0o111) === 0) throw new Error("--cli must identify an executable file");
  const out = openOutputDirectory(options.output);
  const report: Record<string, unknown> = {
    schema: "incodex-tooltip-cold-acceptance-v1",
    startedAt: new Date().toISOString(),
    cli: { path: cli, sha256: sha256(cli) },
    parentPid: options.parentPid,
    toleranceMs: options.toleranceMs,
    measurements: {},
    observations: {},
    rendererUnavailableWarningCount: 0,
    cleanup: { requested: false, method: null, cliExitCode: null, childExited: null, parentAlive: null },
    privacy: { chatOrProfileContentRead: false, screenshots: false, tccChanged: false, sessionFilesDeleted: false },
    result: "FAIL",
  };

  let app: string | null = null;
  let appHashesBefore: { asar: string; infoPlist: string } | null = null;
  let parentStarted: string | null = null;
  let normalPids: number[] = [];
  let child: ReturnType<typeof spawn> | null = null;
  let childExitCode: number | null = null;
  let childExited = false;
  let childPid: number | null = null;
  let cliPid: number | null = null;
  let expectedBinary: string | null = null;
  let ownedChild: DebuggerChild | null = null;
  let target: CdpTarget | null = null;
  let ws: WebSocket | null = null;
  let cdp: ReturnType<typeof cdpClient> | null = null;
  let closeSent = false;
  let closeMethod: string | null = null;
  let errorText: string | null = null;
  let rendererUnavailableWarningCount = 0;
  const rendererPrepareDiagnostics: Array<{ errorCode: string; modulePaths: string[] }> = [];
  const timers = new Set<ReturnType<typeof setTimeout>>();

  const waitChild = (timeoutMs: number): Promise<boolean> => new Promise((resolvePromise) => {
    if (!child || childExited) return resolvePromise(true);
    let done = false;
    const timer = setTimeout(() => {
      timers.delete(timer);
      if (done) return;
      done = true;
      resolvePromise(false);
    }, timeoutMs);
    timers.add(timer);
    child.once("exit", () => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      timers.delete(timer);
      resolvePromise(true);
    });
  });

  const stillOwned = (): boolean => {
    if (!ownedChild || !cliPid || !expectedBinary) return false;
    const snapshot = processRows();
    const current = snapshot.find((row) => row.pid === ownedChild!.pid);
    if (
      !current ||
      current.ppid !== cliPid ||
      current.started !== ownedChild.started ||
      !executableCommand(current, expectedBinary) ||
      !/(?:^|\s)--remote-debugging-port=\d+(?:\s|$)/u.test(current.command)
    ) return false;
    const authorizedPids = verifiedDescendantPids(snapshot, ownedChild.pid);
    const listenerRows = lsofRows(ownedChild.port);
    assertLoopbackListenerOwnership(listenerRows, ownedChild.pid, ownedChild.port, authorizedPids);
    return true;
  };

  const closeExactChild = async (): Promise<void> => {
    if (!ownedChild || !ws || !cdp || childExited) return;
    if (!stillOwned()) throw new Error("exact child ownership changed; refused CDP close");
    await cdp.request("Page.close");
    closeSent = true;
    closeMethod = "Page.close on exact loopback-owned page target";
    report.cleanup = { requested: true, method: "Page.close on exact loopback-owned page target", cliExitCode: childExitCode, childExited: null, parentAlive: null };
  };

  const closeBrowserForExactChild = async (): Promise<void> => {
    if (!ownedChild || !expectedBinary || childExited || !stillOwned()) return;
    const response = await fetch(loopbackUrl(ownedChild.port, "/json/version"), { signal: AbortSignal.timeout(2_000) });
    if (!response.ok) throw new Error(`CDP browser identity returned HTTP ${response.status}`);
    const version = await response.json() as { webSocketDebuggerUrl?: string };
    if (!version.webSocketDebuggerUrl) throw new Error("CDP browser identity has no WebSocket");
    const browserUrl = validateWebSocketUrl(version.webSocketDebuggerUrl, ownedChild.port);
    if (!stillOwned()) throw new Error("child or debugger port ownership changed before browser close");
    const browserSocket = await connectWebSocket(browserUrl.href);
    try {
      await nextMessage(browserSocket, 1, "Browser.close");
      closeSent = true;
      closeMethod = "Browser.close on exact loopback-owned isolated child";
    } finally {
      browserSocket.close();
    }
  };

  try {
    if (!existsSync(dirname(out))) throw new Error("output parent directory does not exist");
    const dryRun = runReadOnly(cli, ["open", "--dry-run"]);
    const candidate = assertInstalledCandidate(cli, dryRun);
    app = candidate.app;
    expectedBinary = candidate.binary;
    appHashesBefore = hashAppFiles(app);
    const processSnapshot = processRows();
    const matchingParents = processSnapshot.filter((row) => executableCommand(row, candidate.binary));
    const parent = matchingParents.find((row) => row.pid === options.parentPid);
    if (!parent || matchingParents.length !== 1) {
      throw new Error(`--parent-pid must identify the only exact running ChatGPT process; found ${matchingParents.length}`);
    }
    parentStarted = parent.started;
    normalPids = matchingParents.map((row) => row.pid);
    Object.assign(report, {
      target: app,
      candidate: statusSummary(candidate.status),
      appHashesBefore,
      normalPidsBefore: normalPids,
      parentProcessStart: parentStarted,
    });

    child = spawn(cli, ["open"], {
      cwd: root,
      env: { ...process.env, NO_COLOR: "1" },
      stdio: "ignore",
    });
    if (!child.pid) throw new Error("could not start incodex open");
    cliPid = child.pid;
    child.once("exit", (code) => { childExitCode = code; childExited = true; });
    const childState = { get exited() { return childExited; }, get exitCode() { return childExitCode; } };

    ownedChild = await waitFor(
      "isolated ChatGPT child owned by this CLI",
      async () => findDebuggerChildIfStarted(processRows(), cliPid!, candidate.binary),
      (value): value is DebuggerChild => value !== null,
      startupTimeoutMs,
      childState,
    );
    const boundChild = ownedChild;
    if (!boundChild) throw new Error("isolated child binding disappeared");
    childPid = boundChild.pid;
    report.process = {
      cliPid,
      childPid,
      childPpid: boundChild.ppid,
      childProcessStart: boundChild.started,
      debuggerPort: boundChild.port,
    };
    await waitFor(
      "loopback CDP listener owned exclusively by the child",
      async () => {
        const snapshot = processRows();
        const rows = lsofRows(boundChild.port);
        if (rows.length === 0) return false;
        const authorizedPids = verifiedDescendantPids(snapshot, boundChild.pid);
        assertLoopbackListenerOwnership(rows, boundChild.pid, boundChild.port, authorizedPids);
        return true;
      },
      (value) => value,
      20_000,
      childState,
    );
    const verifiedListeners = lsofRows(boundChild.port);
    const verifiedProcessTree = verifiedDescendantPids(processRows(), boundChild.pid);
    assertLoopbackListenerOwnership(verifiedListeners, boundChild.pid, boundChild.port, verifiedProcessTree);
    Object.assign(report.process as object, { listenerOwnerPids: [...new Set(verifiedListeners.map((row) => row.pid))] });
    const targets = await waitFor(
      "main app CDP page target",
      async () => {
        try { return uniqueMainPage(await readTargets(boundChild.port)); }
        catch (error) {
          if (String(error).includes("expected one main app page target")) return null;
          if (String(error).includes("timed out")) throw error;
          return null;
        }
      },
      (value): value is CdpTarget => value !== null,
      startupTimeoutMs,
      childState,
    );
    if (!targets) throw new Error("main app target binding disappeared");
    const cdpTarget = targets;
    target = cdpTarget;
    const wsUrl = validateWebSocketUrl(cdpTarget.webSocketDebuggerUrl, boundChild.port);
    ws = await connectWebSocket(wsUrl.href);
    cdp = cdpClient(ws);
    ws.addEventListener("message", (event) => {
      try {
        const message = JSON.parse(String(event.data)) as {
          method?: string;
          params?: { type?: string; args?: Array<{ value?: unknown; description?: string }> };
        };
        if (
          message.method === "Runtime.consoleAPICalled" &&
          message.params?.type === "warning" &&
          message.params.args?.[0]?.value === "[incodex] official tooltip renderer unavailable"
        ) {
          rendererUnavailableWarningCount += 1;
          const detail = message.params.args?.[1]?.value ?? message.params.args?.[1]?.description;
          if (typeof detail === "string" && rendererPrepareDiagnostics.length < 4) {
            rendererPrepareDiagnostics.push(parseRendererPrepareWarning(detail));
          }
        }
      } catch { /* Ignore non-JSON and all unrelated console output. */ }
    });
    await cdp.request("Runtime.enable");
    const childBeforeActivation = processRows().find((row) => row.pid === boundChild.pid);
    if (
      !childBeforeActivation || childBeforeActivation.ppid !== cliPid ||
      childBeforeActivation.started !== boundChild.started ||
      !executableCommand(childBeforeActivation, candidate.binary)
    ) throw new Error("refused to activate a ChatGPT process other than this exact CLI child");
    runReadOnly("/usr/bin/osascript", [
      "-e",
      `tell application "System Events" to set frontmost of first application process whose unix id is ${boundChild.pid} to true`,
    ], 10_000);
    const frontmostPidOutput = runReadOnly("/usr/bin/osascript", [
      "-e",
      "tell application \"System Events\" to get unix id of first application process whose frontmost is true",
    ], 10_000).trim();
    const frontmostPid = Number(frontmostPidOutput);
    if (frontmostPid !== boundChild.pid) throw new Error("the isolated CLI child did not become the frontmost application");
    report.activation = { requestedPid: boundChild.pid, observedFrontmostPid: frontmostPid };
    await cdp.request("Page.bringToFront");
    const initialState = await waitFor(
      "incognito hat and Search controls (renderer readiness is observation only)",
      () => cdp!.evaluate<TooltipState>(tooltipStateExpression()),
      (state) => state.incognito && !!state.button && !!state.search,
      startupTimeoutMs,
      childState,
    );
    if (initialState.titlePresent || initialState.legacyTooltipOpen) {
      throw new Error("native title or legacy tooltip is already present; refusing to measure the wrong tooltip path");
    }
    report.cdp = { targetId: cdpTarget.id, targetUrl: cdpTarget.url, websocketHost: wsUrl.hostname, websocketPort: Number(wsUrl.port) };

    const outPoint = outsidePoint(initialState);
    const move = async (point: { x: number; y: number }) => {
      await cdp!.request("Input.dispatchMouseEvent", { type: "mouseMoved", x: point.x, y: point.y });
    };
    const readState = () => cdp!.evaluate<TooltipState>(tooltipStateExpression());
    const readEvents = () => cdp!.evaluate<ProbeEvent[]>("window.__incodexTooltipAcceptanceEvents||[]");
    const eventSequence = () => cdp!.evaluate<number>("window.__incodexTooltipAcceptanceSequence||0");
    const measureHover = async (
      trigger: "hat" | "search",
      point: { x: number; y: number },
      opens: (state: TooltipState) => boolean,
    ) => {
      const before = await readState();
      const hitTest = trigger === "hat" ? before.hatHitTest : before.searchHitTest;
      if (!before.documentFocused || !hitTest) {
        return { inputConfirmed: false, shown: false, elapsedMs: null, state: before, pointerEnter: null as ProbeEvent | null, reason: !before.documentFocused ? "page document is not focused" : `${trigger} center failed document.elementFromPoint hitTest` };
      }
      const sequenceBefore = await eventSequence();
      await move(point);
      const eventWaitStarted = performance.now();
      const eventWait = await observeUntil(
        readEvents,
        (events) => events.some((event) => event.sequence > sequenceBefore && event.type === "pointerenter" && event.target === trigger && event.trusted),
        1_500,
        childState,
        eventWaitStarted,
      );
      const pointerEnter = eventWait.value.find((event) => event.sequence > sequenceBefore && event.type === "pointerenter" && event.target === trigger && event.trusted) ?? null;
      if (!eventWait.matched || !pointerEnter) {
        return { inputConfirmed: false, shown: false, elapsedMs: null, state: await readState(), pointerEnter: null, reason: `no trusted ${trigger} pointerenter was observed` };
      }
      const tooltipWait = await observeUntil(readState, opens, tooltipTimeoutMs, childState, performance.now());
      return {
        inputConfirmed: true,
        shown: tooltipWait.matched,
        elapsedMs: Math.max(0, Math.round(tooltipWait.value.nowMs - pointerEnter.timeMs)),
        state: tooltipWait.value,
        pointerEnter,
        reason: null as string | null,
      };
    };
    await cdp.evaluate(tooltipEventProbeExpression());
    Object.assign(report.observations as object, { beforeColdHat: stateSummary(initialState) });
    await move(outPoint);
    await waitFor(
      "pointer leaving both tooltip triggers",
      readState,
      (state) => !state.hatHovered && !state.hatTooltipOpen && !state.legacyTooltipVisible && !state.searchTooltipOpen,
      2_000,
      childState,
    );
    await delay(coldIdleMs);

    const hitTestWait = await waitForStableHitTest(readState, childState);
    report.hitTestGate = {
      status: hitTestWait.stable ? "READY" : "INPUT_INVALID",
      waitMs: hitTestWait.waitMs,
      samples: hitTestWait.samples,
      requiredConsecutiveSamples: 3,
      firstBlocked: hitTestWait.firstBlocked ? stateSummary(hitTestWait.firstBlocked as TooltipState) : null,
      final: stateSummary(hitTestWait.state as TooltipState),
    };
    if (!hitTestWait.stable) {
      const reason = "INPUT_INVALID: hat and Search centers did not remain hit-testable while the document was focused; no tooltip timing or Escape dismissal was attempted";
      Object.assign(report.measurements as object, {
        coldFirstHat: { inputStatus: "INPUT_INVALID", elapsedMs: null, tooltipTimeoutMs: null },
        officialSearch: { inputStatus: "INPUT_INVALID", elapsedMs: null, tooltipTimeoutMs: null },
        searchToHat: { inputStatus: "INPUT_INVALID", elapsedMs: null, tooltipTimeoutMs: null },
      });
      report.escape = { inputStatus: "SKIPPED_UNKNOWN_UI", sent: false, reason: "No test-owned tooltip appeared; unknown obstructing UI was left untouched" };
      report.acceptanceFailures = [reason];
      throw new Error(reason);
    }
    const preFirstHat = hitTestWait.state as TooltipState;
    Object.assign(report.observations as object, { hitTestStable: stateSummary(preFirstHat) });
    const coldHatResult = await measureHover(
      "hat",
      { x: initialState.button!.x + initialState.button!.width / 2, y: initialState.button!.y + initialState.button!.height / 2 },
      (state) => state.hatTooltipOpen || state.legacyTooltipVisible,
    );
    const coldHat = coldHatResult.state;
    const coldHatMs = coldHatResult.elapsedMs;
    const coldHatClassHash = classHash(coldHat.hatTooltipClass);
    Object.assign(report.measurements as object, {
      coldFirstHat: {
        inputStatus: coldHatResult.inputConfirmed ? "VALID" : "INPUT_INVALID",
        inputFailure: coldHatResult.reason,
        shown: coldHatResult.inputConfirmed ? coldHatResult.shown : null,
        officialShown: coldHat.hatTooltipOpen,
        legacyFallbackVisible: coldHat.legacyTooltipVisible,
        elapsedMs: coldHatMs,
        tooltipTimeoutMs: coldHatResult.inputConfirmed ? tooltipTimeoutMs : null,
        inputWaitTimeoutMs: coldHatResult.inputConfirmed ? null : 1_500,
        toleranceComparedWithSearchMs: options.toleranceMs,
        officialClassSha256: coldHatClassHash,
        nativeTitlePresent: coldHat.titlePresent,
        duplicateLegacyTooltipOpen: coldHat.legacyTooltipOpen,
      },
    });
    Object.assign(report.observations as object, { afterColdHat: stateSummary(coldHat) });

    const coldCloseStarted = performance.now();
    await move(outPoint);
    const coldClose = await observeUntil(readState, (state) => !state.hatTooltipOpen && !state.legacyTooltipVisible, 2_000, childState, coldCloseStarted);
    Object.assign(report.measurements as object, { coldHatClose: { closed: coldClose.matched, elapsedMs: coldClose.elapsedMs } });
    await delay(coldIdleMs);

    const searchResult = await measureHover(
      "search",
      { x: initialState.search!.x + initialState.search!.width / 2, y: initialState.search!.y + initialState.search!.height / 2 },
      (state) => state.searchTooltipOpen,
    );
    const officialSearch = searchResult.state;
    const searchMs = searchResult.elapsedMs;
    const searchClassHash = classHash(officialSearch.searchTooltipClass);
    Object.assign(report.measurements as object, {
      officialSearch: { inputStatus: searchResult.inputConfirmed ? "VALID" : "INPUT_INVALID", inputFailure: searchResult.reason, shown: searchResult.inputConfirmed ? searchResult.shown : null, elapsedMs: searchMs, tooltipTimeoutMs: searchResult.inputConfirmed ? tooltipTimeoutMs : null, inputWaitTimeoutMs: searchResult.inputConfirmed ? null : 1_500, officialClassSha256: searchClassHash },
    });
    Object.assign(report.observations as object, { afterOfficialSearch: stateSummary(officialSearch) });

    const handoffStarted = performance.now();
    const [handoffResult, searchCloseResult] = await Promise.all([
      measureHover(
        "hat",
        { x: initialState.button!.x + initialState.button!.width / 2, y: initialState.button!.y + initialState.button!.height / 2 },
        (state) => state.hatTooltipOpen || state.legacyTooltipVisible,
      ),
      observeUntil(readState, (state) => !state.searchTooltipOpen, 2_000, childState, handoffStarted),
    ]);
    const handoff = handoffResult.state;
    const handoffMs = handoffResult.elapsedMs;
    const handoffClassHash = classHash(handoff.hatTooltipClass);
    const presentationMatches = handoff.hatTooltipOpen && !!handoffClassHash && !!searchClassHash && handoffClassHash === searchClassHash;
    const directLatencyIssue = coldHatResult.inputConfirmed && coldHat.hatTooltipOpen && searchResult.inputConfirmed && officialSearch.searchTooltipOpen
      ? coldLatencyFailure(coldHatMs!, searchMs!, options.toleranceMs)
      : null;
    const handoffLatencyIssue = handoffResult.inputConfirmed && handoff.hatTooltipOpen && searchResult.inputConfirmed && officialSearch.searchTooltipOpen && handoffMs! - searchMs! > options.toleranceMs
      ? `Search-to-hat handoff was ${handoffMs! - searchMs!}ms slower than Search (tolerance ${options.toleranceMs}ms)`
      : null;
    Object.assign(report.measurements as object, {
      searchClose: { closed: searchCloseResult.matched, elapsedMs: searchCloseResult.elapsedMs },
      searchToHat: { inputStatus: handoffResult.inputConfirmed ? "VALID" : "INPUT_INVALID", inputFailure: handoffResult.reason, shown: handoffResult.inputConfirmed ? handoffResult.shown : null, officialShown: handoff.hatTooltipOpen, legacyFallbackVisible: handoff.legacyTooltipVisible, elapsedMs: handoffMs, tooltipTimeoutMs: handoffResult.inputConfirmed ? tooltipTimeoutMs : null, inputWaitTimeoutMs: handoffResult.inputConfirmed ? null : 1_500, officialClassMatchesSearch: presentationMatches, officialClassSha256: handoffClassHash },
    });
    Object.assign(report.observations as object, { afterSearchToHat: stateSummary(handoff) });

    const stateBeforeEscape = await readState();
    const testOwnedHatTooltipVisible = handoffResult.inputConfirmed && (handoff.hatTooltipOpen || handoff.legacyTooltipVisible);
    const mayDismissTestOwnedTooltip = maySendEscapeToDismissAcceptanceTooltip(
      handoffResult.inputConfirmed,
      testOwnedHatTooltipVisible,
      stateBeforeEscape.documentFocused,
    );
    let escapeTrusted = false;
    let escapeSkipped = false;
    let escapeResult: { matched: boolean; elapsedMs: number; value: TooltipState };
    let readinessAvailable = false;
    let afterEscape: TooltipState;
    if (mayDismissTestOwnedTooltip) {
      const escapeSequenceBefore = await eventSequence();
      const escapeStarted = performance.now();
      await cdp.request("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
      await cdp.request("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
      const escapeInput = await observeUntil(
        readEvents,
        (events) => events.some((event) => event.sequence > escapeSequenceBefore && event.type === "keydown" && event.target === "window" && event.trusted),
        1_000,
        childState,
        escapeStarted,
      );
      escapeTrusted = escapeInput.value.some((event) => event.sequence > escapeSequenceBefore && event.type === "keydown" && event.target === "window" && event.trusted);
      escapeResult = escapeTrusted
        ? await observeUntil(readState, (state) => !state.hatTooltipOpen && !state.legacyTooltipVisible, 2_000, childState, performance.now())
        : { matched: false, elapsedMs: escapeInput.elapsedMs, value: await readState() };
      readinessAvailable = await cdp.evaluate<boolean>("typeof window.__incodexTooltipState?.lifecycle?.presentationReady === 'function'");
      if (escapeTrusted && readinessAvailable) {
        await cdp.evaluate("window.__incodexTooltipState.lifecycle.presentationReady()");
        await delay(250);
      }
      afterEscape = await readState();
    } else {
      escapeSkipped = true;
      escapeResult = { matched: false, elapsedMs: 0, value: stateBeforeEscape };
      afterEscape = stateBeforeEscape;
    }
    const remainedClosed = !afterEscape.hatTooltipOpen && !afterEscape.legacyTooltipVisible;
    report.escape = escapeSkipped
      ? { inputStatus: "SKIPPED_UNKNOWN_UI", sent: false, reason: "No test-owned Search-to-hat tooltip was visible in the focused document; unknown UI was left untouched" }
      : { inputStatus: escapeTrusted ? "VALID" : "INPUT_INVALID", sent: true, dismissed: escapeResult.matched, elapsedMs: escapeResult.elapsedMs, lateReadinessCheckAvailable: readinessAvailable, remainedClosed };
    Object.assign(report.observations as object, { afterEscape: stateSummary(afterEscape) });
    report.rendererUnavailableWarningCount = rendererUnavailableWarningCount;
    report.rendererPrepareDiagnostics = rendererPrepareDiagnostics;
    const acceptanceFailures = [
      ...(!coldHatResult.inputConfirmed ? [`cold-hat timing invalid: ${coldHatResult.reason ?? "trusted pointer input unconfirmed"}`] : []),
      ...(coldHatResult.inputConfirmed && !coldHat.hatTooltipOpen ? [coldHat.legacyTooltipVisible ? "cold hat showed only the legacy fallback, not official Tooltip" : "cold official hat tooltip did not appear before timeout"] : []),
      ...(!searchResult.inputConfirmed ? [`Search timing invalid: ${searchResult.reason ?? "trusted pointer input unconfirmed"}`] : []),
      ...(searchResult.inputConfirmed && !officialSearch.searchTooltipOpen ? ["official Search tooltip did not appear before timeout"] : []),
      ...(!handoffResult.inputConfirmed ? [`Search-to-hat timing invalid: ${handoffResult.reason ?? "trusted pointer input unconfirmed"}`] : []),
      ...(handoffResult.inputConfirmed && !handoff.hatTooltipOpen ? [handoff.legacyTooltipVisible ? "warm hat showed only the legacy fallback, not official Tooltip" : "official Search-to-hat tooltip did not appear before timeout"] : []),
      ...(coldHat.titlePresent || coldHat.legacyTooltipOpen || handoff.titlePresent || handoff.legacyTooltipOpen ? ["native title or duplicate legacy tooltip was present"] : []),
      ...(!presentationMatches && officialSearch.searchTooltipOpen && handoff.hatTooltipOpen ? ["hat tooltip presentation class differs from official Search"] : []),
      ...(directLatencyIssue ? [directLatencyIssue] : []),
      ...(handoffLatencyIssue ? [handoffLatencyIssue] : []),
      ...(escapeSkipped ? ["Escape was skipped because no test-owned tooltip was visible; unknown UI was left untouched"] : []),
      ...(!escapeSkipped && !escapeTrusted ? ["Escape input was not confirmed by trusted keydown"] : []),
      ...(escapeTrusted && (!escapeResult.matched || !remainedClosed) ? ["Escape failed to dismiss or late readiness reopened tooltip"] : []),
    ];
    report.acceptanceFailures = acceptanceFailures;
    const eventLog = await readEvents();
    report.trustedInputEvents = eventLog.slice(-12);
    if (acceptanceFailures.length) throw new Error(acceptanceFailures.join("; "));

    await move(outPoint);
    await waitFor("tooltip triggers to become idle before close", readState, (state) => !state.hatHovered && !state.hatTooltipOpen, 2_000, childState);
    if (!stillOwned()) throw new Error("child or debugger port ownership changed before close");
    await cdp.request("Page.close");
    closeSent = true;
    report.cleanup = { requested: true, method: "Page.close on exact loopback-owned page target", cliExitCode: null, childExited: null, parentAlive: null };
    let appExited = await waitFor(
      "exact isolated ChatGPT child to exit after Page.close",
      async () => !processAlive(ownedChild!.pid),
      (value) => value,
      5_000,
      { exited: false, exitCode: null },
    );
    if (!appExited && stillOwned()) {
      await closeBrowserForExactChild();
      appExited = await waitFor(
        "exact isolated ChatGPT child to exit after Browser.close fallback",
        async () => !processAlive(ownedChild!.pid),
        (value) => value,
        closeTimeoutMs - 5_000,
        { exited: false, exitCode: null },
      );
    }
    if (!appExited) throw new Error("isolated child remained alive after exact Page.close");
    const cliDone = await waitChild(closeTimeoutMs);
    if (!cliDone || childExitCode !== 0) throw new Error(`incodex open did not exit cleanly (code ${childExitCode})`);
    if (processAlive(options.parentPid)) {
      const currentParent = processRows().find((row) => row.pid === options.parentPid);
      if (!currentParent || currentParent.started !== parentStarted || !executableCommand(currentParent, candidate.binary)) {
        throw new Error("the original ChatGPT parent PID no longer matches its captured process identity");
      }
    } else {
      throw new Error("the original ChatGPT parent process exited during acceptance");
    }
    report.result = "PASS";
  } catch (error) {
    errorText = error instanceof Error ? error.message : String(error);
    report.error = errorText;
  } finally {
    for (const timer of timers) clearTimeout(timer);
    report.rendererUnavailableWarningCount = rendererUnavailableWarningCount;
    report.rendererPrepareDiagnostics = rendererPrepareDiagnostics;
    if (ownedChild && processAlive(ownedChild.pid)) {
      try {
        if (stillOwned()) {
          if (target && ws && cdp) {
            try { await closeExactChild(); }
            catch { await closeBrowserForExactChild(); }
          } else {
            await closeBrowserForExactChild();
          }
          const exitedAfterPageClose = await waitFor(
            "exact isolated ChatGPT child to exit during cleanup",
            async () => !processAlive(ownedChild!.pid),
            (value) => value,
            5_000,
            { exited: false, exitCode: null },
          );
          if (!exitedAfterPageClose && stillOwned()) await closeBrowserForExactChild();
        }
      } catch (cleanupError) {
        report.cleanupError = cleanupError instanceof Error ? cleanupError.message : String(cleanupError);
      }
    }
    ws?.close();
    if (child && !childExited) await waitChild(closeTimeoutMs);
    const childStillAlive = childPid ? processAlive(childPid) : false;
    const cliStillAlive = cliPid ? processAlive(cliPid) : false;
    let appHashesAfter: { asar: string; infoPlist: string } | null = null;
    if (app && appHashesBefore) {
      appHashesAfter = hashAppFiles(app);
      report.appHashesAfter = appHashesAfter;
      report.appHashesUnchanged = appHashesBefore.asar === appHashesAfter.asar && appHashesBefore.infoPlist === appHashesAfter.infoPlist;
      if (report.appHashesUnchanged !== true) {
        report.result = "FAIL";
        report.error = "official app package hashes changed during acceptance";
      }
    }
    const parentAfter = options.parentPid ? processRows().find((row) => row.pid === options.parentPid) : undefined;
    report.cleanup = {
      requested: closeSent,
      method: closeSent ? closeMethod : null,
      cliExitCode: childExitCode,
      childPid,
      childExited: !childStillAlive,
      cliExited: !cliStillAlive,
      parentAlive: !!parentAfter && parentAfter.started === parentStarted,
      outputContainsSessionPaths: false,
    };
    const cleanup = report.cleanup as Record<string, unknown>;
    if (childStillAlive || cliStillAlive || cleanup.parentAlive !== true) {
      report.result = "FAIL";
      report.cleanupNeedsManualReview = childStillAlive || cliStillAlive;
    }
    report.finishedAt = new Date().toISOString();
    const evidence = join(out, "tooltip-cold.json");
    writeFileSync(evidence, `${JSON.stringify(report, null, 2)}\n`, { flag: "wx", mode: 0o600 });
    report.evidencePath = evidence;
  }
  if (report.result !== "PASS") throw new Error(errorText ?? "tooltip acceptance failed; see the bounded JSON evidence");
  return report;
}

function selfTest(): void {
  const parsed = parseArguments(["--run", "--cli", "/tmp/incodex", "--parent-pid", "123", "--out", "/tmp/evidence"]);
  if (parsed.mode !== "run" || parsed.parentPid !== 123 || parsed.cli !== "/tmp/incodex") throw new Error("argument parser fixture failed");
  let rejected = false;
  try { parseArguments(["--run", "--cli", "/tmp/incodex", "--parent-pid", "0", "--out", "/tmp/evidence"]); }
  catch { rejected = true; }
  if (!rejected) throw new Error("invalid PID was accepted");
  const targets = parseDryRunTargets("  App          /Applications/ChatGPT.app\n  Binary       /Applications/ChatGPT.app/Contents/MacOS/ChatGPT\n  ! Dry run. No window opened.\n");
  if (targets.binary !== "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT") throw new Error("dry-run target parsing failed");
  const processes = parseProcessTable("  234  123 Sat Sep 26 04:18:00 2026 /Applications/ChatGPT.app/Contents/MacOS/ChatGPT --remote-debugging-port=56789\n");
  const owned = findDebuggerChild(processes, 123, "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT");
  if (owned.pid !== 234 || owned.port !== 56_789) throw new Error("exact CLI child binding failed");
  const listeners = parseListenerRows("COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME\nChatGPT 234 d 60u IPv4 0xabc 0t0 TCP 127.0.0.1:56789 (LISTEN)\n");
  assertLoopbackListenerOwnership(listeners, 234, 56_789);
  for (const unsafe of [
    [{ pid: 999, endpoint: "127.0.0.1:56789" }],
    [{ pid: 234, endpoint: "*:56789" }],
    [{ pid: 234, endpoint: "127.0.0.1:56789" }, { pid: 999, endpoint: "127.0.0.1:56789" }],
  ]) {
    rejected = false;
    try { assertLoopbackListenerOwnership(unsafe, 234, 56_789); } catch { rejected = true; }
    if (!rejected) throw new Error("unsafe CDP ownership fixture was accepted");
  }
  if (validateWebSocketUrl("ws://127.0.0.1:56789/devtools/page/target", 56_789).port !== "56789") throw new Error("loopback WebSocket validation failed");
  rejected = false;
  try { validateWebSocketUrl("ws://192.0.2.1:56789/devtools/page/target", 56_789); } catch { rejected = true; }
  if (!rejected) throw new Error("foreign WebSocket was accepted");
  if (coldLatencyFailure(802, 724, 100) !== null) throw new Error("historical 802ms/724ms timing should pass a 100ms tolerance");
  if (!coldLatencyFailure(1_402, 724, 300)) throw new Error("a repeated cold delay should fail the Search comparison");
  new Function(`return ${tooltipStateExpression()}`);
  new Function(`return ${tooltipEventProbeExpression()}`);
  console.log(JSON.stringify({ ok: true, mode: "no-window-self-test", externalProcessesStarted: false, appTouched: false }));
}

async function main(): Promise<void> {
  const args = Bun.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    console.log(usage());
    return;
  }
  const options = parseArguments(args);
  if (options.mode === "self-test") {
    selfTest();
    return;
  }
  const report = await runAcceptance(options);
  console.log(JSON.stringify({ result: report.result, evidence: report.evidencePath }, null, 2));
}

if (import.meta.main) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
