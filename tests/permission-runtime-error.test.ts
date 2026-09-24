import { expect, test } from "bun:test";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from "node:child_process";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const runDynamic = process.env.INCODEX_RUN_P19_RUNTIME_ERROR === "1";
const nonce = () => createHash("sha256").update(`${process.pid}:${Date.now()}:${Math.random()}`).digest("hex").slice(0, 32);
const digest = (value: Buffer | string) => createHash("sha256").update(value).digest("hex");
const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

type HostEvent = { type: string; message?: string };
type RunningHost = {
  child: ChildProcessWithoutNullStreams;
  nonce: string;
  events: HostEvent[];
  waitFor: (predicate: (event: HostEvent) => boolean, timeoutMs?: number) => Promise<HostEvent>;
  send: (value: Record<string, unknown>) => void;
};

function buildIsolatedFormalHost(directory: string): { executable: string; runtimeDirectory: string; axProbe: string } {
  const native = join(root, "native", "macos");
  const nativeDist = join(native, "dist");
  const runtimeDirectory = join(directory, "runtime");
  mkdirSync(runtimeDirectory, { mode: 0o700 });

  const source = readFileSync(join(native, "permission-views.swift"));
  const nativeManifest = readFileSync(join(nativeDist, "runtime-native-manifest.json"));
  const dylib = readFileSync(join(nativeDist, "incodex-permission-ui.dylib"));
  const host = readFileSync(join(nativeDist, "incodex-permission-host"));
  const manifest = JSON.parse(nativeManifest.toString("utf8"));
  expect(manifest.sourceSha256).toBe(digest(source));
  const hostSources = [
    "permission-host.swift",
    "permission-host-presenter.swift",
    "permission-host-settings.swift",
    "permission-host-flight.swift",
  ].map((name) => readFileSync(join(native, name)));
  expect(manifest.hostSourceSha256).toBe(digest(Buffer.concat(hostSources)));
  expect(manifest.files["incodex-permission-ui.dylib"]).toBe(digest(dylib));
  expect(manifest.files["incodex-permission-host"]).toBe(digest(host));

  writeFileSync(join(runtimeDirectory, "runtime-native-manifest.json"), nativeManifest);
  writeFileSync(join(runtimeDirectory, "incodex-permission-ui.dylib"), dylib, { mode: 0o600 });
  writeFileSync(join(runtimeDirectory, "incodex-permission-host"), host, { mode: 0o700 });
  const runtimeManifest = {
    files: {
      "runtime-native-manifest.json": digest(nativeManifest),
      "incodex-permission-ui.dylib": digest(dylib),
      "incodex-permission-host": digest(host),
    },
  };
  writeFileSync(join(runtimeDirectory, "runtime-manifest.json"), `${JSON.stringify(runtimeManifest)}\n`, { mode: 0o600 });
  chmodSync(runtimeDirectory, 0o700);

  // The product host admits only the unique foreground Codex process. This
  // isolated fixture bypasses just that external-process eligibility check;
  // it still compiles the shipping Swift protocol, presenter, flight and views.
  const hostSource = readFileSync(join(native, "permission-host.swift"), "utf8");
  const gate = /static func canPresentOfficialTarget\(\) -> Bool \{[\s\S]*?\n {4}\}/;
  expect(hostSource.match(gate)?.[0]).toContain("officialCodexBundleIdentifier");
  const isolatedHostSource = hostSource.replace(gate, "static func canPresentOfficialTarget() -> Bool { true\n    }");
  const isolatedHost = join(runtimeDirectory, "permission-host-isolated.swift");
  writeFileSync(isolatedHost, isolatedHostSource);

  const sdk = spawnSync("xcrun", ["--sdk", "macosx", "--show-sdk-path"], { encoding: "utf8" });
  expect(sdk.status, sdk.stderr).toBe(0);
  const architecture = process.arch === "arm64" ? "arm64" : "x86_64";

  const executable = join(runtimeDirectory, "incodex-permission-host-test");
  const hostBuild = spawnSync("xcrun", ["swiftc", "-parse-as-library", "-emit-executable",
    "-module-name", "IncodexPermissionHostIsolatedTest", "-disable-autolinking-runtime-compatibility",
    "-target", `${architecture}-apple-macos12.0`, "-sdk", sdk.stdout.trim(),
    join(native, "permission-views.swift"),
    join(native, "permission-host-settings.swift"),
    join(native, "permission-host-flight.swift"),
    join(native, "permission-host-presenter.swift"),
    isolatedHost,
    "-Xlinker", "-export_dynamic", "-o", executable],
  { cwd: root, encoding: "utf8", timeout: 90_000 });
  expect(hostBuild.status, hostBuild.stderr).toBe(0);
  chmodSync(executable, 0o700);

  const axProbe = join(runtimeDirectory, "permission-runtime-error-ax-smoke");
  const axBuild = spawnSync("clang", ["-fobjc-arc", "-framework", "Cocoa", "-framework", "ApplicationServices",
    "-framework", "CoreGraphics", join(import.meta.dir, "native", "permission-runtime-error-ax-smoke.m"), "-o", axProbe],
  { cwd: root, encoding: "utf8", timeout: 60_000 });
  expect(axBuild.status, axBuild.stderr).toBe(0);
  return { executable, runtimeDirectory, axProbe };
}

function startHost(executable: string, runtimeDirectory: string): RunningHost {
  const hostNonce = nonce();
  const child = spawn(executable, ["--nonce", hostNonce], {
    cwd: runtimeDirectory,
    stdio: ["pipe", "pipe", "pipe"],
    env: process.env,
  });
  const events: HostEvent[] = [];
  let buffer = "";
  let stderr = "";
  child.stdout.setEncoding("utf8");
  child.stderr.setEncoding("utf8");
  child.stdout.on("data", (chunk: string) => {
    buffer += chunk;
    for (;;) {
      const newline = buffer.indexOf("\n");
      if (newline < 0) break;
      const line = buffer.slice(0, newline);
      buffer = buffer.slice(newline + 1);
      try { events.push(JSON.parse(line)); } catch { events.push({ type: "invalid-json", message: line }); }
    }
  });
  child.stderr.on("data", (chunk: string) => { stderr += chunk; });
  const waitFor = async (predicate: (event: HostEvent) => boolean, timeoutMs = 10_000) => {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      const index = events.findIndex(predicate);
      if (index >= 0) return events.splice(index, 1)[0]!;
      if (child.exitCode !== null) throw new Error(`host exited ${child.exitCode}: ${stderr}\nevents=${JSON.stringify(events)}`);
      await delay(20);
    }
    throw new Error(`timed out waiting for host event; stderr=${stderr}; events=${JSON.stringify(events)}`);
  };
  return {
    child, nonce: hostNonce, events, waitFor,
    send: (value) => child.stdin.write(`${JSON.stringify({ nonce: hostNonce, ...value })}\n`),
  };
}

function configure(host: RunningHost, body: string, errorBody: string): void {
  host.send({ type: "configure", layoutDirection: "leftToRight", copy: {
    title: "Enable ChatGPT scripting", body,
    permissionTitle: "Accessibility", permissionDescription: "Read and control app interfaces",
    repair: "Allow", later: "Skip", back: "Back", addedTitle: "ChatGPT added",
    addedBody: "ChatGPT is in the list.", dragInstruction: "Drag ChatGPT into the app list above.",
    completeInSettings: "Complete in Settings", checking: "Checking", repairing: "Preparing System Settings",
    openSettings: "Open Settings", errorTitle: "Permission needs attention", errorBody,
  } });
}

function inspect(axProbe: string, pid: number, title: string, body = "", button = "", press = false): Record<string, any> {
  const result = spawnSync(axProbe, [String(pid), title, body, button, ...(press ? ["press"] : [])], {
    encoding: "utf8", timeout: 10_000,
  });
  expect(result.status, result.stderr).toBe(0);
  return JSON.parse(result.stdout.trim());
}

function runningSettingsPids(): number[] {
  const result = spawnSync("/usr/bin/pgrep", ["-f", "System Settings.app/Contents/MacOS/System Settings"], { encoding: "utf8" });
  if (result.status === 1) return [];
  expect(result.status, result.stderr).toBe(0);
  return result.stdout.trim().split(/\s+/).filter(Boolean).map(Number).filter(Number.isFinite);
}

async function waitForAX(axProbe: string, pid: number, title: string, body = "", button = "", timeoutMs = 8_000) {
  const deadline = Date.now() + timeoutMs;
  let last: Record<string, any> | undefined;
  while (Date.now() < deadline) {
    last = inspect(axProbe, pid, title, body, button);
    if (last.foundWindow && (!body || last.foundBody) && (!button || last.foundButton)) return last;
    await delay(100);
  }
  throw new Error(`AX content did not appear: ${JSON.stringify(last)}`);
}

const longErrorBody = [
  "P19 error recovery diagnostic: the permission repair could not finish.",
  "The application has restored its original permission guide and closed the helper surfaces.",
  "No permission was granted automatically, and this message does not reset any system setting.",
  "Close this window, correct the issue, and start a new explicit permission request from the CLI.",
  "The next request creates a fresh guide and checks the current authorization state again.",
  "If the problem continues, include the request identifier in the diagnostic report.",
  "This deliberately long body verifies natural SwiftUI measurement and full accessibility text.",
  "P19-LONG-BODY-END",
].join("\n");

test.skipIf(process.platform !== "darwin" || !runDynamic)("formal native Swift UI host reflows the P19 error page, cleans helper surfaces, closes on Skip and accepts a fresh request", async () => {
  // The secure native artifact loader deliberately rejects symlink ancestors.
  // Keep the ephemeral test runtime under the canonical repository directory
  // instead of macOS's often-symlinked temporary path.
  const directory = mkdtempSync(join(realpathSync(root), ".p19-runtime-error-"));
  let first: RunningHost | undefined;
  let second: RunningHost | undefined;
  try {
    const built = buildIsolatedFormalHost(directory);
    const settingsBefore = runningSettingsPids();
    expect(settingsBefore.length).toBeGreaterThan(0);
    first = startHost(built.executable, built.runtimeDirectory);
    configure(first, "Allow ChatGPT to access Accessibility.", longErrorBody);
    await first.waitFor((event) => event.type === "ready");

    const initial = await waitForAX(built.axProbe, first.child.pid!, "Enable ChatGPT scripting", "Allow ChatGPT to access Accessibility.", "Allow");
    expect(initial.buttonEnabledKnown).toBe(true);
    expect(initial.buttonEnabled).toBe(true);

    // The existing Settings app is observed through the real locator. This
    // opens only the guide/helper surfaces; no permission toggle or app launch
    // is performed by the fixture.
    first.send({ type: "state", state: "awaiting-user" });
    let awaiting = inspect(built.axProbe, first.child.pid!, "Enable ChatGPT scripting", "", "");
    for (let attempt = 0; attempt < 50 && awaiting.cgOnscreenWindowCount <= initial.cgOnscreenWindowCount; attempt++) {
      await delay(100);
      awaiting = inspect(built.axProbe, first.child.pid!, "Enable ChatGPT scripting", "", "");
    }
    expect(awaiting.cgOnscreenWindowCount).toBeGreaterThan(initial.cgOnscreenWindowCount);

    first.send({ type: "state", state: "error" });
    const error = await waitForAX(built.axProbe, first.child.pid!, "Permission needs attention", longErrorBody, "Allow");
    await delay(600);
    const afterCleanup = inspect(built.axProbe, first.child.pid!, "Permission needs attention", longErrorBody, "Allow");
    expect(error.buttonEnabledKnown).toBe(true);
    expect(error.buttonEnabled).toBe(false);
    expect(error.height).toBeGreaterThan(initial.height);
    expect(error.foundBody).toBe(true);
    // Error state must order out helper/flight/arrow windows and restore the
    // one accessible guide window. AppKit can retain closed offscreen rows in
    // CGWindowList, so compare on-screen rows here and report all rows below.
    expect(afterCleanup.cgOnscreenWindowCount).toBe(initial.cgOnscreenWindowCount);
    expect(afterCleanup.axWindowCount).toBe(initial.axWindowCount);
    const settingsAfter = runningSettingsPids();
    expect(settingsAfter).toEqual(settingsBefore);
    expect(first.events.some((event) => event.type === "allow")).toBe(false);
    expect(afterCleanup.axWindowCount).toBe(initial.axWindowCount);

    const pressed = inspect(built.axProbe, first.child.pid!, "Permission needs attention", longErrorBody, "Skip", true);
    expect(pressed.pressResult).toBe(0);
    await first.waitFor((event) => event.type === "later");
    // The CLI owns the process lifetime: a user dismissal emits `later`, and
    // its caller then sends the close command. The presenter does not invent
    // a second protocol event when it closes its own window.
    first.send({ type: "close" });
    const firstExit = await new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((resolve) =>
      first!.child.once("close", (code, signal) => resolve({ code, signal })));
    expect(firstExit).toEqual({ code: 0, signal: null });

    // A new process and nonce model a fresh explicit CLI request after the
    // dismissed failure. The guide must be recreated and return to initial UI.
    second = startHost(built.executable, built.runtimeDirectory);
    configure(second, "Allow ChatGPT to access Accessibility.", longErrorBody);
    await second.waitFor((event) => event.type === "ready");
    const reentry = await waitForAX(built.axProbe, second.child.pid!, "Enable ChatGPT scripting", "Allow ChatGPT to access Accessibility.", "Allow");
    expect(reentry.buttonEnabled).toBe(true);
    second.send({ type: "close" });
    const secondExit = await new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((resolve) =>
      second!.child.once("close", (code, signal) => resolve({ code, signal })));
    expect(secondExit).toEqual({ code: 0, signal: null });

    console.log(`P19_RUNTIME initial=${initial.width}x${initial.height} awaitingOnscreen=${awaiting.cgOnscreenWindowCount} error=${error.width}x${error.height} errorAX=${afterCleanup.axWindowCount} errorOnscreen=${afterCleanup.cgOnscreenWindowCount} errorCGRows=${JSON.stringify(afterCleanup.cgWindows)} buttonEnabled=${error.buttonEnabled} settingsPidsStable=${settingsBefore.join(",") === settingsAfter.join(",")} allowEvent=false skip=${pressed.pressResult} later=seen closeCommand=sent reentry=yes`);
  } finally {
    for (const host of [first, second]) {
      if (host && host.child.exitCode === null && host.child.signalCode === null) {
        host.child.kill("SIGTERM");
        await delay(100);
      }
    }
    rmSync(directory, { recursive: true, force: true });
  }
}, 180_000);
