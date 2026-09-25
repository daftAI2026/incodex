#!/usr/bin/env bun
import { createHash, randomBytes } from "node:crypto";
import { appendFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from "node:child_process";
import { tmpdir } from "node:os";
import { basename, isAbsolute, join, resolve } from "node:path";
import { ACCESSIBILITY_SETUP_COPY } from "../src/runtime/incognito-copy.ts";
import { resolveLocaleFromCatalog } from "../src/runtime/incodex-locale.cts";
import { sharedPermissionCopy } from "../src/permission-shared-copy.ts";

const repositoryRoot = resolve(import.meta.dir, "..");
const nativeRoot = join(repositoryRoot, "native", "macos");
const hostSourceNames = [
  "permission-views.swift",
  "permission-host-settings.swift",
  "permission-host-flight.swift",
  "permission-host-presenter.swift",
  "permission-host.swift",
] as const;
const helperSourceName = "permission-swift-candidate-visual-helper.swift";
const officialAppPath = "/Applications/ChatGPT.app";
const officialExecutablePath = "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT";
const officialBundleID = "com.openai.codex";
const settingsAppPath = "/System/Applications/System Settings.app";
const settingsExecutablePath = "/System/Applications/System Settings.app/Contents/MacOS/System Settings";

type Options = {
  run: boolean;
  acknowledgeVisibleUI: boolean;
  diagnoseInitialKeyboard: boolean;
  diagnosePlaceholderAXPress: boolean;
  diagnoseSettingsMove: boolean;
  diagnoseSettingsCloseDuringBack: boolean;
  selfTest: boolean;
  help: boolean;
  outputDirectory?: string;
  timeoutSeconds: number;
};

type HostMessage = { nonce: string; type: string; message?: string; locale?: string };
type HelperResult = Record<string, unknown> & { ok?: boolean; error?: string };
type HostEventCounts = { allow: number; retry: number };
type WindowBounds = { x: number; y: number; width: number; height: number };
type SettingsWindow = HelperResult & { pid: number; windowID: number; bounds: WindowBounds; title: string };
type InitialSettingsState = { wasOpen: boolean; pid?: number; window?: SettingsWindow };

const delay = (milliseconds: number) => new Promise((resolvePromise) => setTimeout(resolvePromise, milliseconds));
const timestamp = () => new Date().toISOString();
const monotonicSeconds = () => Number(process.hrtime.bigint()) / 1e9;

function usage(): string {
  return [
    "Swift candidate visual acceptance (diagnostic only; not an install/uninstall test).",
    "",
    "No UI is opened unless both opt-in flags and a new output directory are provided:",
    "  bun scripts/permission-swift-candidate-visual.ts --run --acknowledge-visible-ui --out /private/path/new-run-dir",
    "",
    "The run opens official ChatGPT and System Settings and captures the primary display.",
    "By default it uses one AXPress each for Allow and Back in the temporary candidate host.",
    "Optional: --diagnose-initial-keyboard reads Allow/Skip focus and sends one Return to Allow",
    "in place of AXPress; the helper must see the candidate's normal initial window frontmost.",
    "Optional: --diagnose-placeholder-axpress AXPresses the exact Settings placeholder once",
    "after verifying its AXButton identity, production initial window, and Settings occlusion.",
    "Optional: --diagnose-settings-move moves the exact selected Settings main window by at most 24×16 points during the forward flight, then restores its original rectangle.",
    "Optional: --diagnose-settings-close-during-back AXPresses the selected window's exact AXCloseButton after the reverse flight surface is visible, then restores the original Settings state and rectangle.",
    "These Settings mutations require separate opt-in flags. They never click Settings content or change a permission.",
    "It sends mocked repairing/awaiting-user host states; it never resets or changes TCC.",
    "Full-screen screenshots may contain private desktop content; use a private output path.",
    "Existing ChatGPT/System Settings processes are not terminated. The optional close diagnostic closes one exact Settings window and restores its initial state.",
    "Screen Recording and Accessibility must already be authorized; the script does not request them.",
    "",
    "Safe checks:",
    "  bun scripts/permission-swift-candidate-visual.ts --self-test",
    "  bun scripts/permission-swift-candidate-visual.ts --help",
  ].join("\n");
}

function parseArguments(args: string[]): Options {
  const options: Options = {
    run: false,
    acknowledgeVisibleUI: false,
    diagnoseInitialKeyboard: false,
    diagnosePlaceholderAXPress: false,
    diagnoseSettingsMove: false,
    diagnoseSettingsCloseDuringBack: false,
    selfTest: false,
    help: false,
    timeoutSeconds: 45,
  };
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === "--run") options.run = true;
    else if (argument === "--acknowledge-visible-ui") options.acknowledgeVisibleUI = true;
    else if (argument === "--diagnose-initial-keyboard") options.diagnoseInitialKeyboard = true;
    else if (argument === "--diagnose-placeholder-axpress") options.diagnosePlaceholderAXPress = true;
    else if (argument === "--diagnose-settings-move") options.diagnoseSettingsMove = true;
    else if (argument === "--diagnose-settings-close-during-back") options.diagnoseSettingsCloseDuringBack = true;
    else if (argument === "--self-test") options.selfTest = true;
    else if (argument === "--help" || argument === "-h") options.help = true;
    else if (argument === "--out") {
      const value = args[index + 1];
      if (!value || value.startsWith("--")) throw new Error("--out requires a new output-directory path");
      options.outputDirectory = value;
      index += 1;
    } else if (argument === "--timeout-seconds") {
      const value = Number(args[index + 1]);
      if (!Number.isInteger(value) || value < 15 || value > 180) {
        throw new Error("--timeout-seconds must be an integer from 15 through 180");
      }
      options.timeoutSeconds = value;
      index += 1;
    } else {
      throw new Error(`unknown option: ${argument}`);
    }
  }
  if (options.selfTest && (options.run || options.acknowledgeVisibleUI || options.diagnoseInitialKeyboard || options.diagnosePlaceholderAXPress || options.diagnoseSettingsMove || options.diagnoseSettingsCloseDuringBack || options.outputDirectory)) {
    throw new Error("--self-test cannot be combined with visible-run options");
  }
  if (options.help && (options.run || options.acknowledgeVisibleUI || options.diagnoseInitialKeyboard || options.diagnosePlaceholderAXPress || options.diagnoseSettingsMove || options.diagnoseSettingsCloseDuringBack || options.outputDirectory)) {
    throw new Error("--help cannot be combined with visible-run options");
  }
  return options;
}

function validateRunOptIn(options: Options): string {
  if ((options.diagnoseSettingsMove || options.diagnoseSettingsCloseDuringBack) &&
      (!options.run || !options.acknowledgeVisibleUI || !options.outputDirectory)) {
    throw new Error("Settings window mutation diagnostic requires --run --acknowledge-visible-ui and a new --out directory");
  }
  if (!options.run || !options.acknowledgeVisibleUI) {
    throw new Error("visible run refused: pass both --run and --acknowledge-visible-ui after reviewing the UI/privacy effects");
  }
  if (!options.outputDirectory) throw new Error("visible run refused: --out must name a new, explicit evidence directory");
  if (process.platform !== "darwin") throw new Error("the visible candidate run is supported only on macOS");
  if (!isAbsolute(options.outputDirectory)) throw new Error("--out must be an absolute path");
  const outputDirectory = resolve(options.outputDirectory);
  if (existsSync(outputDirectory)) throw new Error(`--out already exists; refusing to overwrite: ${outputDirectory}`);
  if (basename(outputDirectory) === "" || outputDirectory === "/") throw new Error("--out must be a new, narrow directory path");
  return outputDirectory;
}

function selfTest(): void {
  let rejected = false;
  try {
    validateRunOptIn({ run: true, acknowledgeVisibleUI: false, diagnoseInitialKeyboard: false, diagnosePlaceholderAXPress: false, diagnoseSettingsMove: false, diagnoseSettingsCloseDuringBack: false, selfTest: false, help: false, outputDirectory: "/private/tmp/example", timeoutSeconds: 45 });
  } catch (error) {
    rejected = error instanceof Error && error.message.includes("--acknowledge-visible-ui");
  }
  if (!rejected) throw new Error("missing visible-UI opt-in was not refused");
  assertPlaceholderCallbackCounts({ allow: 1, retry: 0 }, { allow: 1, retry: 1 });
  let callbackMismatchRejected = false;
  try {
    assertPlaceholderCallbackCounts({ allow: 1, retry: 0 }, { allow: 2, retry: 2 });
  } catch {
    callbackMismatchRejected = true;
  }
  if (!callbackMismatchRejected) throw new Error("placeholder callback count mismatch was not refused");
  assertInitialAllowKeyboardResult({
    command: "keyboard-allow",
    keyboardKey: "Return",
    logicalKeyboardPresses: 1,
    allowAXPressPerformed: false,
    allowAXRole: "AXButton",
    allowAXLabel: "Allow",
    allowAXEnabled: true,
    allowAXActionNames: ["AXPress"],
    skipAXRole: "AXButton",
    skipAXLabel: "Skip",
    skipAXEnabled: true,
    skipAXActionNames: ["AXPress"],
    focusedWindowTitleBefore: "Enable ChatGPT scripting",
    expectedWindowTitle: "Enable ChatGPT scripting",
    frontmostPIDBefore: 123,
    targetPID: 123,
  }, "Allow", "Skip");
}

function hostEventCounts(events: HostMessage[]): HostEventCounts {
  return {
    allow: events.filter((event) => event.type === "allow").length,
    retry: events.filter((event) => event.type === "retry").length,
  };
}

function assertPlaceholderCallbackCounts(before: HostEventCounts, after: HostEventCounts): void {
  const allowBefore = before.allow;
  const allowAfter = after.allow;
  const retryBefore = before.retry;
  const retryAfter = after.retry;
  if (retryAfter !== retryBefore + 1) {
    throw new Error(`placeholder AXPress must add exactly one retry callback (before=${retryBefore}, after=${retryAfter})`);
  }
  if (allowAfter !== allowBefore) {
    throw new Error(`placeholder AXPress changed allow callback count (before=${allowBefore}, after=${allowAfter})`);
  }
}

function assertPlaceholderButtonResult(result: HelperResult, expectedLabel: string): void {
  const actions = result.axActionNames as string[] | undefined;
  if (result.role !== "AXButton" || result.buttonAXRole !== "AXButton" || result.uniqueMatchCount !== 1) {
    throw new Error(`placeholder AXPress did not resolve one AXButton: ${JSON.stringify(result)}`);
  }
  if (result.buttonAXTitle !== expectedLabel && result.buttonAXDescription !== expectedLabel && result.buttonAXValue !== expectedLabel) {
    throw new Error(`placeholder AXButton label did not match the selected copy: ${JSON.stringify(result)}`);
  }
  if (result.buttonAXEnabled !== true || !actions?.includes("AXPress") || result.axError !== 0) {
    throw new Error(`placeholder AXButton did not expose and complete AXPress: ${JSON.stringify(result)}`);
  }
}

function assertBackButtonResult(result: HelperResult, expectedLabel: string): void {
  const actions = result.axActionNames as string[] | undefined;
  if (result.role !== "AXButton" || result.buttonAXRole !== "AXButton" || result.uniqueMatchCount !== 1) {
    throw new Error(`Back AXPress did not resolve one AXButton: ${JSON.stringify(result)}`);
  }
  if (result.buttonAXTitle !== expectedLabel && result.buttonAXDescription !== expectedLabel && result.buttonAXValue !== expectedLabel) {
    throw new Error(`Back AXButton label did not match the selected copy: ${JSON.stringify(result)}`);
  }
  if (result.buttonAXEnabled !== true || !actions?.includes("AXPress") || result.axError !== 0) {
    throw new Error(`Back AXButton did not expose and complete AXPress: ${JSON.stringify(result)}`);
  }
}

function assertInitialAllowKeyboardResult(result: HelperResult, expectedAllow: string, expectedSkip: string): void {
  const allowActions = result.allowAXActionNames as string[] | undefined;
  const skipActions = result.skipAXActionNames as string[] | undefined;
  if (result.command !== "keyboard-allow" || result.keyboardKey !== "Return" || result.logicalKeyboardPresses !== 1 || result.allowAXPressPerformed !== false) {
    throw new Error(`initial keyboard diagnostic did not report exactly one Return without AXPress: ${JSON.stringify(result)}`);
  }
  if (result.allowAXRole !== "AXButton" || result.allowAXLabel !== expectedAllow || result.allowAXEnabled !== true || !allowActions?.includes("AXPress")) {
    throw new Error(`initial Allow AX semantics did not match the selected copy: ${JSON.stringify(result)}`);
  }
  if (result.skipAXRole !== "AXButton" || result.skipAXLabel !== expectedSkip || result.skipAXEnabled !== true || !skipActions?.includes("AXPress")) {
    throw new Error(`initial Skip AX semantics did not match the selected copy: ${JSON.stringify(result)}`);
  }
  if (result.focusedWindowTitleBefore !== result.expectedWindowTitle || result.frontmostPIDBefore !== result.targetPID) {
    throw new Error(`initial Return was not scoped to the frontmost candidate initial window: ${JSON.stringify(result)}`);
  }
}

function record(outputDirectory: string, event: Record<string, unknown>): void {
  appendFileSync(join(outputDirectory, "events.jsonl"), `${JSON.stringify({ wallTime: timestamp(), monotonicSeconds: monotonicSeconds(), ...event })}\n`, { mode: 0o600 });
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

function combineErrors(primary: Error | undefined, cleanupLabel: string, cleanup: Error): Error {
  return new Error([primary?.stack ?? primary?.message, `${cleanupLabel}: ${cleanup.stack ?? cleanup.message}`].filter(Boolean).join("\n"));
}

function run(command: string, args: string[], timeoutMilliseconds = 30_000): string {
  const result = spawnSync(command, args, { cwd: repositoryRoot, encoding: "utf8", timeout: timeoutMilliseconds });
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`.trim();
  if (result.error) throw new Error(`${command} failed to start: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`${command} exited ${result.status ?? "without status"}: ${output || "no output"}`);
  return result.stdout ?? "";
}

function helperCall(helper: string, args: string[], timeoutMilliseconds = 10_000): HelperResult {
  const stdout = run(helper, args, timeoutMilliseconds);
  const line = stdout.trim().split("\n").at(-1);
  if (!line) throw new Error(`visual helper ${args[0]} returned no JSON`);
  const result = JSON.parse(line) as HelperResult;
  if (result.ok !== true) throw new Error(String(result.error ?? `visual helper ${args[0]} failed`));
  return result;
}

function createDirectory(path: string): void {
  mkdirSync(path, { recursive: false, mode: 0o700 });
  const actual = realpathSync(path);
  record(path, { type: "evidence-directory-created", path, realPath: actual, permissions: "0700" });
  writeFileSync(join(path, ".incodex-diagnostic-mocked-state"), "This is visual diagnostic evidence only; no install/uninstall or TCC reset was performed.\n", { mode: 0o600, flag: "wx" });
}

function compileCandidate(tempDirectory: string): { host: string; helper: string } {
  const architecture = process.arch === "arm64" ? "arm64" : "x86_64";
  const host = join(tempDirectory, "incodex-permission-host-swift-candidate");
  const helper = join(tempDirectory, "permission-swift-candidate-visual-helper");
  const hostSources = hostSourceNames.map((name) => join(nativeRoot, name));
  run("xcrun", [
    "swiftc", "-parse-as-library", "-emit-executable", "-target", `${architecture}-apple-macos12.0`,
    "-module-name", "IncodexPermissionHostVisualAudit", ...hostSources, "-o", host,
  ], 180_000);
  run("xcrun", [
    "swiftc", "-parse-as-library", "-emit-executable", "-target", `${architecture}-apple-macos12.0`,
    "-module-name", "PermissionSwiftCandidateVisualHelper", join(import.meta.dir, helperSourceName),
    "-framework", "AppKit", "-framework", "ApplicationServices", "-framework", "CoreGraphics", "-framework", "ScreenCaptureKit",
    "-o", helper,
  ], 120_000);
  return { host, helper };
}

function sha256(path: string): string {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function makeCopy(appLocale: string): { locale: string; direction: "leftToRight" | "rightToLeft"; copy: Record<string, string> } {
  const resolved = resolveLocaleFromCatalog(appLocale, ACCESSIBILITY_SETUP_COPY);
  const catalog = sharedPermissionCopy(ACCESSIBILITY_SETUP_COPY) as Record<string, Record<string, string>>;
  const entry = catalog[resolved];
  if (!entry?.officialBody) throw new Error(`official-app guide copy is unavailable for resolved app locale ${resolved}`);
  const { officialBody, ...remaining } = entry;
  const copy = { ...remaining, body: officialBody };
  for (const [key, value] of Object.entries(copy)) {
    if (typeof value !== "string" || value.length > 4096) throw new Error(`guide copy field ${key} is not a bounded string`);
  }
  const language = resolved.toLowerCase().split("-")[0];
  return { locale: resolved, direction: ["ar", "fa", "ur"].includes(language) ? "rightToLeft" : "leftToRight", copy };
}

function sendHost(child: ChildProcessWithoutNullStreams, nonce: string, value: Record<string, unknown>): void {
  if (!child.stdin.writable) throw new Error("candidate host input has closed");
  child.stdin.write(`${JSON.stringify({ nonce, ...value })}\n`);
}

function createHostClient(child: ChildProcessWithoutNullStreams, nonce: string, outputDirectory: string) {
  const events: HostMessage[] = [];
  const waiters: Array<{ predicate: (message: HostMessage) => boolean; resolve: (message: HostMessage) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }> = [];
  let stdoutBuffer = "";
  let stderrBuffer = "";
  let exited: { code: number | null; signal: NodeJS.Signals | null } | undefined;
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdin.on("error", () => {});
  child.stdout.on("data", (chunk: string) => {
    stdoutBuffer += chunk;
    for (;;) {
      const newline = stdoutBuffer.indexOf("\n");
      if (newline < 0) break;
      const raw = stdoutBuffer.slice(0, newline);
      stdoutBuffer = stdoutBuffer.slice(newline + 1);
      if (!raw) continue;
      let message: HostMessage;
      try { message = JSON.parse(raw) as HostMessage; } catch { record(outputDirectory, { type: "host-invalid-output", raw }); continue; }
      record(outputDirectory, { type: "host-event", hostEvent: message });
      events.push(message);
      const index = waiters.findIndex((waiter) => waiter.predicate(message));
      if (index >= 0) {
        const [waiter] = waiters.splice(index, 1);
        clearTimeout(waiter.timer);
        waiter.resolve(message);
      }
    }
  });
  child.stderr.on("data", (chunk: string) => {
    stderrBuffer += chunk;
    appendFileSync(join(outputDirectory, "host-stderr.log"), chunk, { mode: 0o600 });
  });
  child.once("close", (code, signal) => {
    exited = { code, signal };
    record(outputDirectory, { type: "host-exit", code, signal, stderrTail: stderrBuffer.slice(-4000) });
    for (const waiter of waiters.splice(0)) {
      clearTimeout(waiter.timer);
      waiter.reject(new Error(`candidate host exited before expected event (code=${code}, signal=${signal})`));
    }
  });
  return {
    events,
    send: (value: Record<string, unknown>) => sendHost(child, nonce, value),
    waitFor(predicate: (message: HostMessage) => boolean, timeoutMilliseconds: number): Promise<HostMessage> {
      const existing = events.find(predicate);
      if (existing) return Promise.resolve(existing);
      if (exited) return Promise.reject(new Error(`candidate host already exited (code=${exited.code}, signal=${exited.signal})`));
      return new Promise((resolvePromise, reject) => {
        const waiter = {
          predicate,
          resolve: resolvePromise,
          reject,
          timer: setTimeout(() => {
            const index = waiters.indexOf(waiter);
            if (index >= 0) waiters.splice(index, 1);
            reject(new Error(`timed out after ${timeoutMilliseconds}ms waiting for candidate host event`));
          }, timeoutMilliseconds),
        };
        waiters.push(waiter);
      });
    },
    hasExited: () => exited !== undefined,
  };
}

function currentWindows(helper: string, outputDirectory: string, label: string): HelperResult {
  const result = helperCall(helper, ["windows"]);
  record(outputDirectory, { type: "window-snapshot", label, result });
  return result;
}

async function takeScreenshot(helper: string, outputDirectory: string, label: string, index: number): Promise<void> {
  const path = join(outputDirectory, `${String(index).padStart(3, "0")}-${label}.png`);
  const startedAt = monotonicSeconds();
  const result = helperCall(helper, ["snapshot", path], 15_000);
  record(outputDirectory, { type: "screenshot", label, path, callStartMonotonicSeconds: startedAt, result });
}

async function captureSeries(helper: string, outputDirectory: string, phase: string, count: number, intervalMilliseconds: number): Promise<void> {
  const child = spawn(helper, ["series", outputDirectory, phase, String(count), String(intervalMilliseconds)], { stdio: ["ignore", "pipe", "pipe"] });
  let stdoutBuffer = "";
  let stderrBuffer = "";
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk: string) => {
    stdoutBuffer += chunk;
    for (;;) {
      const newline = stdoutBuffer.indexOf("\n");
      if (newline < 0) break;
      const raw = stdoutBuffer.slice(0, newline);
      stdoutBuffer = stdoutBuffer.slice(newline + 1);
      if (!raw) continue;
      try {
        const frame = JSON.parse(raw) as HelperResult;
        record(outputDirectory, { type: frame.ok === true ? "screenshot" : "capture-error", phase, sequenceIndex: frame.seriesIndex, result: frame });
        if (frame.ok !== true) child.kill("SIGTERM");
      } catch (error) {
        record(outputDirectory, { type: "capture-invalid-output", phase, raw, error: String(error) });
        child.kill("SIGTERM");
      }
    }
  });
  child.stderr.on("data", (chunk: string) => {
    stderrBuffer += chunk;
    appendFileSync(join(outputDirectory, "capture-stderr.log"), chunk, { mode: 0o600 });
  });
  const code = await new Promise<number | null>((resolvePromise, reject) => {
    const timer = setTimeout(() => {
      child.kill("SIGTERM");
      reject(new Error(`capture series ${phase} exceeded 30 seconds`));
    }, 30_000);
    child.once("error", (error) => { clearTimeout(timer); reject(error); });
    child.once("close", (exitCode) => { clearTimeout(timer); resolvePromise(exitCode); });
  });
  const trailing = stdoutBuffer.trim();
  if (trailing) {
    try {
      const frame = JSON.parse(trailing) as HelperResult;
      record(outputDirectory, { type: frame.ok === true ? "screenshot" : "capture-error", phase, sequenceIndex: frame.seriesIndex, result: frame });
    } catch (error) {
      record(outputDirectory, { type: "capture-invalid-output", phase, raw: trailing, error: String(error) });
      throw error;
    }
  }
  if (code !== 0) throw new Error(`capture series ${phase} exited ${code}: ${stderrBuffer.slice(-2000)}`);
  await delay(0);
}

async function waitForOfficialApp(helper: string, outputDirectory: string, timeoutMilliseconds: number): Promise<number> {
  run("/usr/bin/open", ["-a", officialAppPath]);
  record(outputDirectory, { type: "open-request", app: officialAppPath, purpose: "activate official frontmost gate; no app content is clicked" });
  const deadline = Date.now() + timeoutMilliseconds;
  let latest: HelperResult | undefined;
  while (Date.now() < deadline) {
    latest = helperCall(helper, ["app", officialBundleID, officialExecutablePath]);
    const pids = latest.matchingPIDs as number[] | undefined;
    const active = latest.activePIDs as number[] | undefined;
    if (pids?.length === 1 && active?.length === 1 && latest.frontmostPID === pids[0]) {
      const windows = latest.windows as Array<Record<string, unknown>> | undefined;
      if (windows?.some((window) => window.layer === 0 && (window.alpha as number) > 0)) {
        record(outputDirectory, { type: "official-app-ready", state: latest });
        return pids[0]!;
      }
    }
    await delay(250);
  }
  throw new Error(`official ChatGPT did not become the unique active frontmost app with a visible layer-0 window; last status=${JSON.stringify(latest)}`);
}

async function waitForSettings(helper: string, outputDirectory: string, timeoutMilliseconds: number): Promise<number> {
  if (!existsSync(settingsAppPath)) throw new Error(`System Settings app not found at ${settingsAppPath}`);
  run("/usr/bin/open", ["-a", settingsAppPath]);
  record(outputDirectory, { type: "open-request", app: settingsAppPath, purpose: "real Settings window only; no pane or permission switch is clicked" });
  const deadline = Date.now() + timeoutMilliseconds;
  let latest: HelperResult | undefined;
  while (Date.now() < deadline) {
    latest = helperCall(helper, ["app", "com.apple.systempreferences", settingsExecutablePath]);
    const pids = latest.matchingPIDs as number[] | undefined;
    if (pids?.length === 1) {
      const windows = latest.windows as Array<Record<string, unknown>> | undefined;
      const mains = windows?.filter((window) => {
        const bounds = window.bounds as Record<string, number> | undefined;
        return window.layer === 0 && (window.alpha as number) > 0 && (bounds?.width ?? 0) > 600 && (bounds?.height ?? 0) >= 470;
      });
      if (mains && mains.length >= 1) {
        record(outputDirectory, { type: "settings-ready", pid: pids[0], mainWindows: mains, state: latest });
        return pids[0]!;
      }
    }
    await delay(200);
  }
  throw new Error(`System Settings did not expose a visible main window; last status=${JSON.stringify(latest)}`);
}

function settingsWindow(helper: string, outputDirectory: string, pid: number, label: string): SettingsWindow {
  const result = helperCall(helper, ["settings-window", String(pid), settingsExecutablePath]) as SettingsWindow;
  if (result.pid !== pid || typeof result.windowID !== "number" || !result.bounds || typeof result.title !== "string") {
    throw new Error(`Settings did not resolve to one exact AXWindow/CGWindow identity: ${JSON.stringify(result)}`);
  }
  record(outputDirectory, { type: "settings-window-identity", label, result });
  return result;
}

function initialSettingsState(helper: string, outputDirectory: string, enabled: boolean): InitialSettingsState {
  if (!enabled) return { wasOpen: false };
  const app = helperCall(helper, ["app", "com.apple.systempreferences", settingsExecutablePath]);
  const pids = app.matchingPIDs as number[] | undefined;
  if (!pids?.length) {
    const state = { wasOpen: false } satisfies InitialSettingsState;
    record(outputDirectory, { type: "settings-original-state", state });
    return state;
  }
  if (pids.length !== 1) throw new Error(`Settings mutation diagnostic requires one exact System Settings process; found ${pids.length}`);
  const pid = pids[0]!;
  const visible = (app.windows as Array<Record<string, unknown>> | undefined)?.filter((window) => window.layer === 0 && (window.alpha as number) > 0) ?? [];
  if (!visible.length) {
    const state = { wasOpen: false, pid } satisfies InitialSettingsState;
    record(outputDirectory, { type: "settings-original-state", state, note: "process existed but had no visible layer-0 window" });
    return state;
  }
  if (visible.length !== 1) throw new Error(`Settings mutation diagnostic requires one visible Settings window; found ${visible.length}`);
  const window = settingsWindow(helper, outputDirectory, pid, "before-test");
  const state = { wasOpen: true, pid, window } satisfies InitialSettingsState;
  record(outputDirectory, { type: "settings-original-state", state });
  return state;
}

function moveSettingsWindow(helper: string, outputDirectory: string, window: SettingsWindow, label: string): HelperResult {
  const startedAt = monotonicSeconds();
  const result = helperCall(helper, [
    "settings-move",
    String(window.pid), settingsExecutablePath, String(window.windowID),
    String(window.bounds.x), String(window.bounds.y), String(window.bounds.width), String(window.bounds.height),
  ]);
  record(outputDirectory, { type: "settings-window-moved", label, callStartMonotonicSeconds: startedAt, expectedWindow: window, result });
  return result;
}

function restoreSettingsPosition(helper: string, outputDirectory: string, window: SettingsWindow, target: WindowBounds, label: string): HelperResult {
  const startedAt = monotonicSeconds();
  const current = settingsWindow(helper, outputDirectory, window.pid, `${label}-before`);
  if (current.windowID !== window.windowID) throw new Error(`Settings window identity changed before restoring its position: expected ${window.windowID}, received ${current.windowID}`);
  const result = helperCall(helper, [
    "settings-restore-position",
    String(current.pid), settingsExecutablePath, String(current.windowID),
    String(current.bounds.x), String(current.bounds.y), String(current.bounds.width), String(current.bounds.height),
    String(target.x), String(target.y),
  ]);
  record(outputDirectory, { type: "settings-window-position-restored", label, callStartMonotonicSeconds: startedAt, expectedWindow: current, targetBounds: target, result });
  return result;
}

function closeSettingsWindow(helper: string, outputDirectory: string, window: SettingsWindow, label: string): HelperResult {
  const startedAt = monotonicSeconds();
  const result = helperCall(helper, [
    "settings-close",
    String(window.pid), settingsExecutablePath, String(window.windowID),
    String(window.bounds.x), String(window.bounds.y), String(window.bounds.width), String(window.bounds.height),
  ]);
  record(outputDirectory, { type: "settings-window-closed", label, callStartMonotonicSeconds: startedAt, expectedWindow: window, result });
  return result;
}

async function waitForVisibleFlightStart(
  helper: string,
  outputDirectory: string,
  hostPID: number,
  baseline: HelperResult,
  direction: "forward" | "reverse",
  timeoutMilliseconds: number,
): Promise<HelperResult> {
  const baselineWindows = baseline.windows as Array<Record<string, unknown>> | undefined;
  const baselineFlightIDs = new Set((baselineWindows ?? [])
    .filter((window) => window.ownerPID === hostPID && window.layer === 25 && (window.alpha as number) > 0)
    .map((window) => window.windowID));
  const deadline = Date.now() + Math.min(timeoutMilliseconds, 2_000);
  let latest: HelperResult | undefined;
  while (Date.now() < deadline) {
    latest = helperCall(helper, ["windows"]);
    const windows = latest.windows as Array<Record<string, unknown>> | undefined;
    const flightWindow = windows?.find((window) =>
      window.ownerPID === hostPID && window.layer === 25 && (window.alpha as number) > 0 &&
      typeof window.windowID === "number" && !baselineFlightIDs.has(window.windowID));
    if (flightWindow) {
      record(outputDirectory, { type: `${direction}-flight-visible-start`, hostPID, flightWindow, result: latest });
      return latest;
    }
    await delay(40);
  }
  throw new Error(`${direction} flight screen surface was not observed; last window snapshot=${JSON.stringify(latest)}`);
}

async function restoreInitialSettingsState(
  helper: string,
  outputDirectory: string,
  original: InitialSettingsState,
  timeoutMilliseconds: number,
): Promise<void> {
  const app = helperCall(helper, ["app", "com.apple.systempreferences", settingsExecutablePath]);
  const pids = app.matchingPIDs as number[] | undefined;
  let pid = pids?.length === 1 ? pids[0]! : original.pid;
  const visible = (app.windows as Array<Record<string, unknown>> | undefined)?.filter((window) => window.layer === 0 && (window.alpha as number) > 0) ?? [];
  if (pids && pids.length > 1) throw new Error(`cannot restore Settings state: found ${pids.length} System Settings processes`);

  if (original.wasOpen) {
    if (!original.window) throw new Error("cannot restore Settings state: original visible window identity was not recorded");
    if (!visible.length) {
      run("/usr/bin/open", ["-a", settingsAppPath]);
      record(outputDirectory, { type: "settings-reopened-for-restore", app: settingsAppPath, priorPID: original.pid });
      pid = await waitForSettings(helper, outputDirectory, timeoutMilliseconds);
    } else if (visible.length !== 1) {
      throw new Error(`cannot restore Settings state: expected one visible main window, found ${visible.length}`);
    }
    const currentPID = pid ?? (await waitForSettings(helper, outputDirectory, timeoutMilliseconds));
    const current = settingsWindow(helper, outputDirectory, currentPID, "before-final-restore");
    if (Math.abs(current.bounds.width - original.window.bounds.width) > 1 || Math.abs(current.bounds.height - original.window.bounds.height) > 1) {
      throw new Error(`Settings window size changed; position-only restore cannot reproduce original rectangle: original=${JSON.stringify(original.window.bounds)} current=${JSON.stringify(current.bounds)}`);
    }
    if (Math.abs(current.bounds.x - original.window.bounds.x) > 1 || Math.abs(current.bounds.y - original.window.bounds.y) > 1) {
      restoreSettingsPosition(helper, outputDirectory, current, original.window.bounds, "restore-preexisting-settings-rectangle");
    }
    record(outputDirectory, { type: "settings-original-state-restored", wasOpen: true, original: original.window, finalWindow: settingsWindow(helper, outputDirectory, currentPID, "after-final-restore") });
    return;
  }

  if (!visible.length) {
    record(outputDirectory, { type: "settings-original-state-restored", wasOpen: false, nowHasVisibleWindow: false, processPID: pid ?? null });
    return;
  }
  if (visible.length !== 1 || !pid) throw new Error(`cannot restore initially closed Settings state: visible window/process identity is not unique (${visible.length} window(s), PID=${pid ?? "none"})`);
  const current = settingsWindow(helper, outputDirectory, pid, "before-close-initially-closed-settings");
  closeSettingsWindow(helper, outputDirectory, current, "restore-initially-closed-settings");
  const afterClose = helperCall(helper, ["app", "com.apple.systempreferences", settingsExecutablePath]);
  const remaining = (afterClose.windows as Array<Record<string, unknown>> | undefined)?.filter((window) => window.layer === 0 && (window.alpha as number) > 0) ?? [];
  if (remaining.length !== 0) throw new Error(`Settings remained visibly open after restoring initial closed state: ${JSON.stringify(remaining)}`);
  record(outputDirectory, { type: "settings-original-state-restored", wasOpen: false, nowHasVisibleWindow: false, processPID: pid });
}

async function waitForHostWindowCount(helper: string, outputDirectory: string, pid: number, expectedMaximum: number, timeoutMilliseconds: number): Promise<void> {
  const deadline = Date.now() + timeoutMilliseconds;
  let latest: HelperResult | undefined;
  while (Date.now() < deadline) {
    latest = currentWindows(helper, outputDirectory, `host-pid-${pid}-count-check`);
    const windows = latest.windows as Array<Record<string, unknown>> | undefined;
    const count = windows?.filter((window) => window.ownerPID === pid && (window.alpha as number) > 0).length ?? 0;
    if (count <= expectedMaximum) {
      record(outputDirectory, { type: "host-window-count-reached", pid, count, expectedMaximum });
      return;
    }
    await delay(150);
  }
  throw new Error(`candidate PID ${pid} did not settle to at most ${expectedMaximum} visible windows: ${JSON.stringify(latest)}`);
}

async function waitForHostInitialReturn(helper: string, outputDirectory: string, pid: number, timeoutMilliseconds: number): Promise<HelperResult> {
  const deadline = Date.now() + timeoutMilliseconds;
  let latest: HelperResult | undefined;
  while (Date.now() < deadline) {
    latest = currentWindows(helper, outputDirectory, `host-pid-${pid}-initial-return-check`);
    const windows = latest.windows as Array<Record<string, unknown>> | undefined;
    const visible = windows?.filter((window) => window.ownerPID === pid && (window.alpha as number) > 0) ?? [];
    const initial = visible.filter((window) => {
      const bounds = window.bounds as WindowBounds | undefined;
      // restoreInitialPage() deliberately raises the initial NSWindow to
      // .floating before disposing the helper. Back must settle at layer 3,
      // including when the Settings window closes during the reverse flight.
      return window.layer === 3 && bounds && Math.abs(bounds.width - 600) <= 2 && bounds.height >= 250 && bounds.height <= 450;
    });
    const initialIDs = new Set(initial.map((window) => window.windowID));
    const transient = visible.filter((window) => window.layer === 25 ||
      (window.layer === 3 && !initialIDs.has(window.windowID)));
    if (visible.length === 1 && initial.length === 1 && transient.length === 0) {
      record(outputDirectory, { type: "host-initial-page-restored", pid, result: latest, passed: true });
      return latest;
    }
    await delay(80);
  }
  throw new Error(`candidate host did not return to exactly one visible floating initial 600-point window: ${JSON.stringify(latest)}`);
}

function createManifest(outputDirectory: string, values: Record<string, unknown>): void {
  writeFileSync(join(outputDirectory, "result.json"), `${JSON.stringify({
    kind: "diagnostic-mocked-state",
    scope: "Swift candidate native-host visual acceptance only",
    installUninstallAcceptance: false,
    tccResetPerformed: false,
    permissionSwitchChanged: false,
    localeConfigurationChanged: false,
    installedApplicationsChanged: false,
    manualT01DragIncluded: false,
    fullPrimaryDisplayCapture: true,
    outputDirectory,
    ...values,
    finishedAt: timestamp(),
  }, null, 2)}\n`, { mode: 0o600, flag: "wx" });
}

async function visibleRun(options: Options): Promise<void> {
  const outputDirectory = validateRunOptIn(options);
  if (!existsSync(officialAppPath) || !existsSync(officialExecutablePath)) {
    throw new Error(`official target is unavailable at ${officialAppPath}; no substitute target will be used`);
  }
  if (!existsSync(settingsAppPath) || !existsSync(settingsExecutablePath)) throw new Error("System Settings.app is unavailable");
  const tempDirectory = mkdtempSync(join(tmpdir(), "incodex-swift-candidate-visual-"));
  let hostChild: ChildProcessWithoutNullStreams | undefined;
  let outputCreated = false;
  let hostClient: ReturnType<typeof createHostClient> | undefined;
  let visualHelperPath: string | undefined;
  let originalSettings: InitialSettingsState | undefined;
  let resultStatus: "running" | "passed-diagnostic-only" | "failed-diagnostic-only" = "running";
  let errorText: string | undefined;
  let runFailure: Error | undefined;
  let settingsRestoreFailure: Error | undefined;
  try {
    const binaries = compileCandidate(tempDirectory);
    visualHelperPath = binaries.helper;
    const preflight = helperCall(binaries.helper, ["preflight"]);
    if (preflight.screenCaptureAlreadyGranted !== true) throw new Error("Screen Recording is not already authorized for this helper; refused before app launch, no request was made");
    if (preflight.accessibilityAlreadyGranted !== true) throw new Error("Accessibility control is not already authorized for this helper; refused before app launch, no request was made");
    if (options.diagnoseInitialKeyboard && preflight.postEventAccessAlreadyGranted !== true) {
      throw new Error("CGPreflightPostEventAccess is false for the keyboard diagnostic helper; refused before app launch, no permission prompt was requested");
    }
    if (Number(String(preflight.osVersion).split(".")[0]) < 14) throw new Error("this read-only screen-capture harness requires macOS 14 or later");

    createDirectory(outputDirectory);
    outputCreated = true;
    originalSettings = initialSettingsState(
      binaries.helper,
      outputDirectory,
      options.diagnoseSettingsMove || options.diagnoseSettingsCloseDuringBack,
    );
    const sourcePaths = [...hostSourceNames.map((name) => join(nativeRoot, name)), join(import.meta.dir, helperSourceName), join(import.meta.dir, "permission-swift-candidate-visual.ts")];
    const sourceHashes = Object.fromEntries(sourcePaths.map((path) => [path.replace(`${repositoryRoot}/`, ""), sha256(path)]));
    const commit = run("git", ["rev-parse", "HEAD"]).trim();
    const arch = process.arch === "arm64" ? "arm64" : "x86_64";
    writeFileSync(join(outputDirectory, "manifest.json"), `${JSON.stringify({
      kind: "diagnostic-mocked-state",
      scope: "Swift candidate native-host visual acceptance only",
      status: resultStatus,
      installUninstallAcceptance: false,
      tccResetPerformed: false,
      permissionSwitchChanged: false,
      localeConfigurationChanged: false,
      installedApplicationsChanged: false,
      manualT01DragIncluded: false,
      initialKeyboardDiagnostic: options.diagnoseInitialKeyboard,
      placeholderAXPressDiagnostic: options.diagnosePlaceholderAXPress,
      settingsMoveDiagnostic: options.diagnoseSettingsMove,
      settingsCloseDuringBackDiagnostic: options.diagnoseSettingsCloseDuringBack,
      settingsInitialState: originalSettings ?? null,
      fullPrimaryDisplayCapture: true,
      outputDirectory,
      repoHead: commit,
      architecture: arch,
      hostBuildDefines: [],
      officialGate: "unmodified; ChatGPT must be unique, active, frontmost, and have a visible layer-0 window",
      screenCapturePreflight: preflight,
      sourceSha256: sourceHashes,
      hostBinarySha256: sha256(binaries.host),
      helperBinarySha256: sha256(binaries.helper),
      startedAt: timestamp(),
    }, null, 2)}\n`, { mode: 0o600, flag: "wx" });
    record(outputDirectory, { type: "preflight-passed", result: preflight, repoHead: commit, architecture: arch, sourceSha256: sourceHashes });

    const chatGPTPid = await waitForOfficialApp(binaries.helper, outputDirectory, options.timeoutSeconds * 1000);
    record(outputDirectory, { type: "target-gate-satisfied", bundleIdentifier: officialBundleID, pid: chatGPTPid, executable: officialExecutablePath });

    const nonce = randomBytes(16).toString("hex");
    const childEnvironment = { ...process.env };
    delete childEnvironment.INCODEX_PERMISSION_HOST_TESTING;
    delete childEnvironment.INCODEX_PERMISSION_HOST_STUB;
    delete childEnvironment.INCODEX_PERMISSION_HOST_DISABLE_PRESENTATION;
    delete childEnvironment.INCODEX_PERMISSION_HOST_TARGET_BUNDLE;
    hostChild = spawn(binaries.host, ["--nonce", nonce], {
      cwd: repositoryRoot,
      env: childEnvironment,
      stdio: ["pipe", "pipe", "pipe"],
    });
    if (!hostChild.pid) throw new Error("candidate host failed to obtain a PID");
    hostClient = createHostClient(hostChild, nonce, outputDirectory);
    record(outputDirectory, { type: "host-started", pid: hostChild.pid, executable: binaries.host, noncePrefix: nonce.slice(0, 8), buildDefines: [] });

    hostClient.send({ type: "app-locale-request" });
    const localeEvent = await hostClient.waitFor((message) => message.type === "app-locale" || message.type === "error", options.timeoutSeconds * 1000);
    if (localeEvent.type === "error") throw new Error(`candidate host locale probe failed: ${localeEvent.message ?? "unknown error"}`);
    if (!localeEvent.locale) throw new Error("candidate host did not report the official app locale");
    const selected = makeCopy(localeEvent.locale);
    record(outputDirectory, { type: "copy-selected", appLocale: localeEvent.locale, resolvedLocale: selected.locale, copyContext: "official-restored-app", layoutDirection: selected.direction });
    hostClient.send({ type: "configure", copy: selected.copy, layoutDirection: selected.direction });
    const ready = await hostClient.waitFor((message) => message.type === "ready" || message.type === "error", options.timeoutSeconds * 1000);
    if (ready.type === "error") throw new Error(`candidate host failed to present: ${ready.message ?? "unknown error"}`);
    await takeScreenshot(binaries.helper, outputDirectory, "initial-ready", 1);

    const allowLabel = selected.copy.repair ?? "Allow";
    const skipLabel = selected.copy.later ?? "Skip";
    const initialCountsBefore = hostEventCounts(hostClient.events);
    if (initialCountsBefore.allow !== 0 || initialCountsBefore.retry !== 0 || hostClient.events.some((event) => event.type === "later")) {
      throw new Error(`initial permission page already emitted an action before Allow: ${JSON.stringify(initialCountsBefore)}`);
    }
    if (options.diagnoseInitialKeyboard) {
      const keyboardAllow = helperCall(binaries.helper, [
        "keyboard-allow",
        String(hostChild.pid),
        allowLabel,
        skipLabel,
        selected.copy.title ?? "Enable ChatGPT scripting",
      ]);
      assertInitialAllowKeyboardResult(keyboardAllow, allowLabel, skipLabel);
      record(outputDirectory, {
        type: "initial-keyboard-allow",
        hostPID: hostChild.pid,
        callbackCountsBefore: initialCountsBefore,
        result: keyboardAllow,
        axPressPerformed: false,
      });
    } else {
      const allowPress = helperCall(binaries.helper, ["press", String(hostChild.pid), allowLabel]);
      record(outputDirectory, { type: "ax-button-pressed", label: allowLabel, hostPID: hostChild.pid, result: allowPress });
    }
    const allowEvent = await hostClient.waitFor((message) => ["allow", "later", "retry", "error"].includes(message.type), options.timeoutSeconds * 1000);
    if (allowEvent.type === "error") throw new Error(`candidate host reported error after Allow: ${allowEvent.message ?? "unknown error"}`);
    if (allowEvent.type !== "allow") throw new Error(`expected exactly one Allow event, received ${allowEvent.type}`);
    await delay(200);
    const initialCountsAfter = hostEventCounts(hostClient.events);
    const initialLaterCount = hostClient.events.filter((event) => event.type === "later").length;
    if (initialCountsAfter.allow !== 1 || initialCountsAfter.retry !== 0 || initialLaterCount !== 0) {
      throw new Error(`initial Allow action did not produce exactly one Allow callback: before=${JSON.stringify(initialCountsBefore)} after=${JSON.stringify(initialCountsAfter)} later=${initialLaterCount}`);
    }
    record(outputDirectory, {
      type: "initial-allow-callback-check",
      action: options.diagnoseInitialKeyboard ? "Return keyboard event; no Allow AXPress" : "one Allow AXPress",
      before: initialCountsBefore,
      after: initialCountsAfter,
      later: initialLaterCount,
      passed: true,
    });

    hostClient.send({ type: "state", state: "repairing" });
    record(outputDirectory, { type: "mocked-state", state: "repairing", reason: "diagnostic harness substitutes only the CLI/TCC operation" });
    await takeScreenshot(binaries.helper, outputDirectory, "repairing-mocked", 2);
    const settingsPid = await waitForSettings(binaries.helper, outputDirectory, options.timeoutSeconds * 1000);

    const settingsWindowBeforeForward = options.diagnoseSettingsMove
      ? settingsWindow(binaries.helper, outputDirectory, settingsPid, "before-forward-flight")
      : undefined;
    const forwardBaseline = options.diagnoseSettingsMove
      ? currentWindows(binaries.helper, outputDirectory, "before-forward-flight")
      : undefined;
    if (settingsWindowBeforeForward && originalSettings?.wasOpen && originalSettings.window) {
      const original = originalSettings.window.bounds;
      const current = settingsWindowBeforeForward.bounds;
      if (Math.abs(current.x - original.x) > 1 || Math.abs(current.y - original.y) > 1 ||
          Math.abs(current.width - original.width) > 1 || Math.abs(current.height - original.height) > 1) {
        throw new Error(`Settings moved before the forward-flight diagnostic: original=${JSON.stringify(original)} current=${JSON.stringify(current)}`);
      }
    }
    const forwardSeries = captureSeries(binaries.helper, outputDirectory, "allow-to-settings-helper", 12, 100)
      .then(() => undefined, (error) => asError(error));
    let settingsMoveAttempted = false;
    let forwardError: Error | undefined;
    try {
      hostClient.send({ type: "state", state: "awaiting-user" });
      record(outputDirectory, { type: "mocked-state", state: "awaiting-user", settingsPID: settingsPid, reason: "opened real System Settings app; no pane or permission row was clicked" });
      if (options.diagnoseSettingsMove && settingsWindowBeforeForward) {
        if (forwardBaseline) {
          await waitForVisibleFlightStart(binaries.helper, outputDirectory, hostChild.pid, forwardBaseline, "forward", options.timeoutSeconds * 1000);
        }
        settingsMoveAttempted = true;
        moveSettingsWindow(binaries.helper, outputDirectory, settingsWindowBeforeForward, "during-forward-flight");
      }
      const captureError = await forwardSeries;
      if (captureError) throw captureError;
      await takeScreenshot(binaries.helper, outputDirectory, "helper-landed", 30);
      currentWindows(binaries.helper, outputDirectory, "helper-landed-window-order");
    } catch (error) {
      forwardError = asError(error);
    } finally {
      const captureError = await forwardSeries;
      if (captureError) {
        record(outputDirectory, { type: "forward-capture-settlement-error", error: captureError.stack ?? captureError.message });
        if (forwardError !== captureError) forwardError = combineErrors(forwardError, "forward capture settlement failed", captureError);
      }
      if (settingsMoveAttempted && settingsWindowBeforeForward) {
        try {
          restoreSettingsPosition(
            binaries.helper,
            outputDirectory,
            settingsWindowBeforeForward,
            settingsWindowBeforeForward.bounds,
            "finally-restore-after-forward-flight-move",
          );
        } catch (error) {
          const restoreError = asError(error);
          record(outputDirectory, { type: "forward-settings-restore-error", error: restoreError.stack ?? restoreError.message });
          forwardError = combineErrors(forwardError, "forward Settings position restore failed", restoreError);
        }
        settingsMoveAttempted = false;
      }
    }
    if (forwardError) throw forwardError;

    let placeholderDiagnosticError: Error | undefined;
    let placeholderCountsBefore: HostEventCounts | undefined;
    if (options.diagnosePlaceholderAXPress) {
      placeholderCountsBefore = hostEventCounts(hostClient.events);
      try {
        if (placeholderCountsBefore.allow !== 1 || placeholderCountsBefore.retry !== 0) {
          throw new Error(`placeholder diagnostic requires the initial Allow-only callback state: ${JSON.stringify(placeholderCountsBefore)}`);
        }
        const placeholderLabel = selected.copy.completeInSettings ?? "Complete in System Settings";
        const placeholderPress = helperCall(binaries.helper, ["press-settings-placeholder", String(hostChild.pid), placeholderLabel]);
        assertPlaceholderButtonResult(placeholderPress, placeholderLabel);
        record(outputDirectory, { type: "placeholder-axpress", hostPID: hostChild.pid, label: placeholderLabel, result: placeholderPress });
        const retryEvent = await hostClient.waitFor(
          (message) => message.type === "retry" || message.type === "error",
          5_000,
        );
        if (retryEvent.type === "error") throw new Error(`candidate host reported an error after placeholder AXPress: ${retryEvent.message ?? "unknown error"}`);
        await delay(250);
        const callbackCounts = hostEventCounts(hostClient.events);
        assertPlaceholderCallbackCounts(placeholderCountsBefore, callbackCounts);
        record(outputDirectory, { type: "placeholder-axpress-callback-check", before: placeholderCountsBefore, after: callbackCounts, passed: true });
      } catch (error) {
        placeholderDiagnosticError = error instanceof Error ? error : new Error(String(error));
        record(outputDirectory, {
          type: "placeholder-axpress-callback-check",
          before: placeholderCountsBefore,
          after: hostEventCounts(hostClient.events),
          passed: false,
          error: placeholderDiagnosticError.message,
        });
      }
    }

    const backTitle = selected.copy.back ?? "Back";
    const settingsWindowBeforeBackClose = options.diagnoseSettingsCloseDuringBack
      ? settingsWindow(binaries.helper, outputDirectory, settingsPid, "before-back-flight-close")
      : undefined;
    const backBaseline = options.diagnoseSettingsCloseDuringBack
      ? currentWindows(binaries.helper, outputDirectory, "before-back-flight")
      : undefined;
    const backPress = helperCall(binaries.helper, ["press", String(hostChild.pid), backTitle]);
    assertBackButtonResult(backPress, backTitle);
    record(outputDirectory, { type: "ax-button-pressed", label: backTitle, hostPID: hostChild.pid, result: backPress });
    const reverseSeries = captureSeries(binaries.helper, outputDirectory, "back-to-initial", 12, 100)
      .then(() => undefined, (error) => asError(error));
    let reverseError: Error | undefined;
    try {
      if (options.diagnoseSettingsCloseDuringBack && settingsWindowBeforeBackClose && backBaseline) {
        await waitForVisibleFlightStart(binaries.helper, outputDirectory, hostChild.pid, backBaseline, "reverse", options.timeoutSeconds * 1000);
        const exactCurrentSettingsWindow = settingsWindow(binaries.helper, outputDirectory, settingsPid, "at-back-flight-close");
        if (exactCurrentSettingsWindow.windowID !== settingsWindowBeforeBackClose.windowID ||
            Math.abs(exactCurrentSettingsWindow.bounds.x - settingsWindowBeforeBackClose.bounds.x) > 1 ||
            Math.abs(exactCurrentSettingsWindow.bounds.y - settingsWindowBeforeBackClose.bounds.y) > 1 ||
            Math.abs(exactCurrentSettingsWindow.bounds.width - settingsWindowBeforeBackClose.bounds.width) > 1 ||
            Math.abs(exactCurrentSettingsWindow.bounds.height - settingsWindowBeforeBackClose.bounds.height) > 1) {
          throw new Error(`Settings window identity or bounds changed before Back-close: before=${JSON.stringify(settingsWindowBeforeBackClose)} now=${JSON.stringify(exactCurrentSettingsWindow)}`);
        }
        const closeResult = helperCall(binaries.helper, [
          "settings-close-during-flight",
          String(exactCurrentSettingsWindow.pid), settingsExecutablePath, String(exactCurrentSettingsWindow.windowID),
          String(exactCurrentSettingsWindow.bounds.x), String(exactCurrentSettingsWindow.bounds.y),
          String(exactCurrentSettingsWindow.bounds.width), String(exactCurrentSettingsWindow.bounds.height),
          String(hostChild.pid),
        ]);
        record(outputDirectory, { type: "settings-back-close", label: "after-visible-reverse-flight-start", expectedWindow: exactCurrentSettingsWindow, result: closeResult });
      }
      const captureError = await reverseSeries;
      if (captureError) throw captureError;
      if (options.diagnoseSettingsCloseDuringBack) {
        await waitForHostInitialReturn(binaries.helper, outputDirectory, hostChild.pid, options.timeoutSeconds * 1000);
      } else {
        await waitForHostWindowCount(binaries.helper, outputDirectory, hostChild.pid, 1, options.timeoutSeconds * 1000);
      }
      await takeScreenshot(binaries.helper, outputDirectory, "initial-restored", 60);
      currentWindows(binaries.helper, outputDirectory, "initial-restored-window-order");
    } catch (error) {
      reverseError = asError(error);
    } finally {
      const captureError = await reverseSeries;
      if (captureError) {
        record(outputDirectory, { type: "reverse-capture-settlement-error", error: captureError.stack ?? captureError.message });
        if (reverseError !== captureError) reverseError = combineErrors(reverseError, "reverse capture settlement failed", captureError);
      }
    }
    if (reverseError) throw reverseError;
    if (placeholderCountsBefore) {
      const finalCounts = hostEventCounts(hostClient.events);
      try {
        assertPlaceholderCallbackCounts(placeholderCountsBefore, finalCounts);
        record(outputDirectory, { type: "placeholder-axpress-final-callback-check", before: placeholderCountsBefore, after: finalCounts, passed: true });
      } catch (error) {
        placeholderDiagnosticError ??= error instanceof Error ? error : new Error(String(error));
        record(outputDirectory, { type: "placeholder-axpress-final-callback-check", before: placeholderCountsBefore, after: finalCounts, passed: false, error: placeholderDiagnosticError.message });
      }
    }
    if (placeholderDiagnosticError) throw placeholderDiagnosticError;
    resultStatus = "passed-diagnostic-only";
  } catch (error) {
    resultStatus = "failed-diagnostic-only";
    runFailure = asError(error);
    errorText = runFailure.stack ?? runFailure.message;
    if (outputCreated) record(outputDirectory, { type: "runner-error", error: errorText });
  } finally {
    if (hostChild && hostChild.exitCode === null && hostChild.signalCode === null) {
      try {
        hostClient?.send({ type: "close" });
        record(outputDirectory, { type: "cleanup-close-sent", hostPID: hostChild.pid });
      } catch (error) {
        record(outputDirectory, { type: "cleanup-close-failed", hostPID: hostChild.pid, error: String(error) });
      }
      const closed = await Promise.race([new Promise<boolean>((resolvePromise) => hostChild?.once("close", () => resolvePromise(true))), delay(2_500).then(() => false)]);
      if (!closed && hostChild.pid && hostChild.exitCode === null && hostChild.signalCode === null) {
        record(outputDirectory, { type: "cleanup-sigterm-own-child", hostPID: hostChild.pid });
        hostChild.kill("SIGTERM");
        const terminated = await Promise.race([new Promise<boolean>((resolvePromise) => hostChild?.once("close", () => resolvePromise(true))), delay(2_500).then(() => false)]);
        if (!terminated && hostChild.pid && hostChild.exitCode === null && hostChild.signalCode === null) {
          record(outputDirectory, { type: "cleanup-sigkill-own-child", hostPID: hostChild.pid });
          hostChild.kill("SIGKILL");
        }
      }
    }
    if (outputCreated && visualHelperPath && originalSettings &&
        (options.diagnoseSettingsMove || options.diagnoseSettingsCloseDuringBack)) {
      try {
        await restoreInitialSettingsState(visualHelperPath, outputDirectory, originalSettings, options.timeoutSeconds * 1000);
      } catch (error) {
        settingsRestoreFailure = error instanceof Error ? error : new Error(String(error));
        resultStatus = "failed-diagnostic-only";
        errorText = [errorText, `Settings state restoration failed: ${settingsRestoreFailure.message}`].filter(Boolean).join("\n");
        record(outputDirectory, { type: "settings-original-state-restore-failed", error: settingsRestoreFailure.stack ?? settingsRestoreFailure.message });
      }
    }
    if (outputCreated) {
      try {
        createManifest(outputDirectory, {
          status: resultStatus,
          error: errorText,
          initialKeyboardDiagnostic: options.diagnoseInitialKeyboard,
          placeholderAXPressDiagnostic: options.diagnosePlaceholderAXPress,
          settingsMoveDiagnostic: options.diagnoseSettingsMove,
          settingsCloseDuringBackDiagnostic: options.diagnoseSettingsCloseDuringBack,
          settingsInitialState: originalSettings ?? null,
        });
      } catch (error) {
        record(outputDirectory, { type: "manifest-finalization-error", error: String(error) });
      }
    }
    rmSync(tempDirectory, { recursive: true, force: true });
  }
  if (settingsRestoreFailure) throw combineErrors(runFailure, "Settings restoration failed", settingsRestoreFailure);
  if (runFailure) throw runFailure;
}

async function main(): Promise<void> {
  let options: Options;
  try {
    options = parseArguments(process.argv.slice(2));
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n\n${usage()}\n`);
    process.exitCode = 2;
    return;
  }
  if (options.help) {
    process.stdout.write(`${usage()}\n`);
    return;
  }
  if (options.selfTest) {
    try {
      selfTest();
      process.stdout.write("permission-swift-candidate-visual self-test passed; no UI or permission APIs were used\n");
    } catch (error) {
      process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
      process.exitCode = 1;
    }
    return;
  }
  try {
    validateRunOptIn(options);
  } catch (error) {
    process.stderr.write(`permission-swift-candidate-visual refused: ${error instanceof Error ? error.message : String(error)}\n\n${usage()}\n`);
    process.exitCode = 2;
    return;
  }
  try {
    process.stdout.write("Opt-in accepted. This will open ChatGPT and System Settings and save full primary-display screenshots. No permission pane will be clicked; Settings window changes run only with their explicit diagnostic flags.\n");
    await visibleRun(options);
    process.stdout.write(`Diagnostic visual run finished. Evidence retained at ${options.outputDirectory}\n`);
  } catch (error) {
    process.stderr.write(`permission-swift-candidate-visual refused or failed: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}

if (import.meta.main) void main();
