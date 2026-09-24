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
  selfTest: boolean;
  help: boolean;
  outputDirectory?: string;
  timeoutSeconds: number;
};

type HostMessage = { nonce: string; type: string; message?: string; locale?: string };
type HelperResult = Record<string, unknown> & { ok?: boolean; error?: string };

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
    "The run opens official ChatGPT and System Settings, presses only the unique AXButton",
    "Allow and Back in the temporary candidate host, and captures the primary display.",
    "It sends mocked repairing/awaiting-user host states; it never resets or changes TCC.",
    "Full-screen screenshots may contain private desktop content; use a private output path.",
    "Existing ChatGPT/System Settings processes are not terminated. Only the child host is cleaned up.",
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
    selfTest: false,
    help: false,
    timeoutSeconds: 45,
  };
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    if (argument === "--run") options.run = true;
    else if (argument === "--acknowledge-visible-ui") options.acknowledgeVisibleUI = true;
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
  if (options.selfTest && (options.run || options.acknowledgeVisibleUI || options.outputDirectory)) {
    throw new Error("--self-test cannot be combined with visible-run options");
  }
  if (options.help && (options.run || options.acknowledgeVisibleUI || options.outputDirectory)) {
    throw new Error("--help cannot be combined with visible-run options");
  }
  return options;
}

function validateRunOptIn(options: Options): string {
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
    validateRunOptIn({ run: true, acknowledgeVisibleUI: false, selfTest: false, help: false, outputDirectory: "/private/tmp/example", timeoutSeconds: 45 });
  } catch (error) {
    rejected = error instanceof Error && error.message.includes("--acknowledge-visible-ui");
  }
  if (!rejected) throw new Error("missing visible-UI opt-in was not refused");
}

function record(outputDirectory: string, event: Record<string, unknown>): void {
  appendFileSync(join(outputDirectory, "events.jsonl"), `${JSON.stringify({ wallTime: timestamp(), monotonicSeconds: monotonicSeconds(), ...event })}\n`, { mode: 0o600 });
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
  let resultStatus: "running" | "passed-diagnostic-only" | "failed-diagnostic-only" = "running";
  let errorText: string | undefined;
  try {
    const binaries = compileCandidate(tempDirectory);
    const preflight = helperCall(binaries.helper, ["preflight"]);
    if (preflight.screenCaptureAlreadyGranted !== true) throw new Error("Screen Recording is not already authorized for this helper; refused before app launch, no request was made");
    if (preflight.accessibilityAlreadyGranted !== true) throw new Error("Accessibility control is not already authorized for this helper; refused before app launch, no request was made");
    if (Number(String(preflight.osVersion).split(".")[0]) < 14) throw new Error("this read-only screen-capture harness requires macOS 14 or later");

    createDirectory(outputDirectory);
    outputCreated = true;
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

    const allowPress = helperCall(binaries.helper, ["press", String(hostChild.pid), selected.copy.repair ?? "Allow"]);
    record(outputDirectory, { type: "ax-button-pressed", label: "Allow", hostPID: hostChild.pid, result: allowPress });
    const allowEvent = await hostClient.waitFor((message) => ["allow", "error"].includes(message.type), options.timeoutSeconds * 1000);
    if (allowEvent.type === "error") throw new Error(`candidate host reported error after Allow: ${allowEvent.message ?? "unknown error"}`);
    if (allowEvent.type !== "allow") throw new Error(`expected Allow event, received ${allowEvent.type}`);

    hostClient.send({ type: "state", state: "repairing" });
    record(outputDirectory, { type: "mocked-state", state: "repairing", reason: "diagnostic harness substitutes only the CLI/TCC operation" });
    await takeScreenshot(binaries.helper, outputDirectory, "repairing-mocked", 2);
    const settingsPid = await waitForSettings(binaries.helper, outputDirectory, options.timeoutSeconds * 1000);

    const forwardSeries = captureSeries(binaries.helper, outputDirectory, "allow-to-settings-helper", 12, 100);
    hostClient.send({ type: "state", state: "awaiting-user" });
    record(outputDirectory, { type: "mocked-state", state: "awaiting-user", settingsPID: settingsPid, reason: "opened real System Settings app; no pane or permission row was clicked" });
    await forwardSeries;
    await takeScreenshot(binaries.helper, outputDirectory, "helper-landed", 30);
    currentWindows(binaries.helper, outputDirectory, "helper-landed-window-order");

    const backTitle = selected.copy.back ?? "Back";
    const backPress = helperCall(binaries.helper, ["press", String(hostChild.pid), backTitle]);
    record(outputDirectory, { type: "ax-button-pressed", label: backTitle, hostPID: hostChild.pid, result: backPress });
    await captureSeries(binaries.helper, outputDirectory, "back-to-initial", 12, 100);
    await waitForHostWindowCount(binaries.helper, outputDirectory, hostChild.pid, 1, options.timeoutSeconds * 1000);
    await takeScreenshot(binaries.helper, outputDirectory, "initial-restored", 60);
    currentWindows(binaries.helper, outputDirectory, "initial-restored-window-order");
    resultStatus = "passed-diagnostic-only";
  } catch (error) {
    resultStatus = "failed-diagnostic-only";
    errorText = error instanceof Error ? error.stack ?? error.message : String(error);
    if (outputCreated) record(outputDirectory, { type: "runner-error", error: errorText });
    throw error;
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
    if (outputCreated) {
      try {
        createManifest(outputDirectory, { status: resultStatus, error: errorText });
      } catch (error) {
        record(outputDirectory, { type: "manifest-finalization-error", error: String(error) });
      }
    }
    rmSync(tempDirectory, { recursive: true, force: true });
  }
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
    process.stdout.write("Opt-in accepted. This will open ChatGPT and System Settings and save full primary-display screenshots. No permission pane will be clicked, and only the candidate host process will be closed.\n");
    await visibleRun(options);
    process.stdout.write(`Diagnostic visual run finished. Evidence retained at ${options.outputDirectory}\n`);
  } catch (error) {
    process.stderr.write(`permission-swift-candidate-visual refused or failed: ${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  }
}

if (import.meta.main) void main();
