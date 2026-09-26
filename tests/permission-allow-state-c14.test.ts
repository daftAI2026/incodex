import { expect, test } from "bun:test";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from "node:child_process";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";

const root = join(import.meta.dir, "..");
const runDiagnostic = process.env.INCODEX_RUN_C14_ALLOW_STATE === "1";
const nonce = () => createHash("sha256").update(`${process.pid}:${Date.now()}:${Math.random()}`).digest("hex").slice(0, 32);
const digest = (value: Buffer | string) => createHash("sha256").update(value).digest("hex");
const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

type HostEvent = { type: string; message?: string };
type RunningHost = {
  child: ChildProcessWithoutNullStreams;
  events: HostEvent[];
  allEvents: HostEvent[];
  waitFor: (predicate: (event: HostEvent) => boolean, timeoutMs?: number) => Promise<HostEvent>;
  send: (value: Record<string, unknown>) => void;
};
type Probe = Record<string, any>;

function buildIsolatedFormalHost(directory: string): { executable: string; runtimeDirectory: string; probe: string } {
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
  // isolated fixture bypasses that external eligibility gate and uses the
  // same SwiftUI views, native presenter, flight and Settings locator.
  const hostSource = readFileSync(join(native, "permission-host.swift"), "utf8");
  const gate = /static func canPresentOfficialTarget\(\) -> Bool \{[\s\S]*?\n {4}\}/;
  expect(hostSource.match(gate)?.[0]).toContain("officialCodexBundleIdentifier");
  const isolatedHostSource = hostSource.replace(gate, "static func canPresentOfficialTarget() -> Bool { true\n    }");
  const aquaMarker = "let application = NSApplication.shared\n        application.setActivationPolicy(.accessory)";
  expect(isolatedHostSource).toContain(aquaMarker);
  const aquaHostSource = isolatedHostSource.replace(
    aquaMarker,
    "let application = NSApplication.shared\n        application.appearance = NSAppearance(named: .aqua)\n        application.setActivationPolicy(.accessory)",
  );
  const isolatedHost = join(runtimeDirectory, "permission-host-isolated.swift");
  writeFileSync(isolatedHost, aquaHostSource);

  const sdk = spawnSync("xcrun", ["--sdk", "macosx", "--show-sdk-path"], { encoding: "utf8" });
  expect(sdk.status, sdk.stderr).toBe(0);
  const architecture = process.arch === "arm64" ? "arm64" : "x86_64";

  const executable = join(runtimeDirectory, "incodex-permission-host-c14");
  const hostBuild = spawnSync("xcrun", ["swiftc", "-parse-as-library", "-emit-executable",
    "-module-name", "IncodexPermissionHostC14Test", "-disable-autolinking-runtime-compatibility",
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

  const probe = join(runtimeDirectory, "permission-allow-state-c14");
  const probeBuild = spawnSync("clang", ["-fobjc-arc", "-framework", "Cocoa", "-framework", "ApplicationServices",
    "-framework", "CoreGraphics", "-framework", "ScreenCaptureKit",
    join(import.meta.dir, "native", "permission-allow-state-c14.m"), "-o", probe],
  { cwd: root, encoding: "utf8", timeout: 60_000 });
  expect(probeBuild.status, probeBuild.stderr).toBe(0);
  return { executable, runtimeDirectory, probe };
}

function startHost(executable: string, runtimeDirectory: string): RunningHost {
  const hostNonce = nonce();
  const child = spawn(executable, ["--nonce", hostNonce], {
    cwd: runtimeDirectory,
    stdio: ["pipe", "pipe", "pipe"],
    env: process.env,
  });
  const events: HostEvent[] = [];
  const allEvents: HostEvent[] = [];
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
      try {
        const event = JSON.parse(line) as HostEvent;
        events.push(event);
        allEvents.push(event);
      } catch {
        const event = { type: "invalid-json", message: line };
        events.push(event);
        allEvents.push(event);
      }
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
    child,
    events,
    allEvents,
    waitFor,
    send: (value) => child.stdin.write(`${JSON.stringify({ nonce: hostNonce, ...value })}\n`),
  };
}

function configure(host: RunningHost): void {
  host.send({ type: "configure", layoutDirection: "leftToRight", copy: {
    title: "Enable ChatGPT scripting", body: "Allow ChatGPT to access Accessibility.",
    permissionTitle: "Accessibility", permissionDescription: "Read and control app interfaces",
    repair: "Allow", later: "Skip", back: "Back", addedTitle: "ChatGPT added",
    addedBody: "ChatGPT is in the list.", dragInstruction: "Drag ChatGPT into the app list above.",
    completeInSettings: "Complete in Settings", checking: "Checking", repairing: "Preparing System Settings",
    openSettings: "Open Settings", errorTitle: "Permission needs attention", errorBody: "The permission request needs attention.",
  } });
}

function probe(executable: string, pid: number, mode: string, snapshotPath?: string): Probe {
  const args = [String(pid), "Enable ChatGPT scripting", "Allow", mode];
  if (snapshotPath) args.push(snapshotPath);
  const result = spawnSync(executable, args, { encoding: "utf8", timeout: 10_000 });
  expect(result.status, result.stderr).toBe(0);
  return JSON.parse(result.stdout.trim());
}

async function waitForGuide(executable: string, pid: number, timeoutMs = 8_000): Promise<Probe> {
  const deadline = Date.now() + timeoutMs;
  let last: Probe | undefined;
  while (Date.now() < deadline) {
    const result = spawnSync(executable, [String(pid), "Enable ChatGPT scripting", "Allow", "inspect"], {
      encoding: "utf8", timeout: 10_000,
    });
    if (result.status === 0) {
      last = JSON.parse(result.stdout.trim()) as Probe;
      if (last.foundWindow && last.foundButton) return last;
    }
    await delay(100);
  }
  throw new Error(`AX guide did not appear: ${JSON.stringify(last)}`);
}

function runningSettingsPids(): number[] {
  const result = spawnSync("/usr/bin/pgrep", ["-f", "System Settings.app/Contents/MacOS/System Settings"], { encoding: "utf8" });
  if (result.status === 1) return [];
  expect(result.status, result.stderr).toBe(0);
  return result.stdout.trim().split(/\s+/).filter(Boolean).map(Number).filter(Number.isFinite);
}

function luminance(pixel: Probe): number {
  const { r, g, b } = pixel.rgb;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

test.skipIf(process.platform !== "darwin" || !runDiagnostic)(
  "formal native Swift UI host records the C14 Allow control through hover, held press, drag-out cancellation and one recovery click",
  async () => {
    // The secure native artifact loader rejects symlink ancestors. The isolated
    // runtime lives under the real repo. Captured PNGs stay in the OS temp dir
    // during the run; INCODEX_C14_EVIDENCE_DIR opts into keeping them after it.
    const directory = mkdtempSync(join(realpathSync(root), ".c14-allow-state-"));
    const evidenceRoot = process.env.INCODEX_C14_EVIDENCE_DIR?.trim()
      ? resolve(process.env.INCODEX_C14_EVIDENCE_DIR.trim())
      : undefined;
    if (evidenceRoot) mkdirSync(evidenceRoot, { recursive: true });
    const snapshotDirectory = mkdtempSync(join(evidenceRoot ?? tmpdir(), "c14-allow-state-captures-"));
    let host: RunningHost | undefined;
    let pointerHeld = false;
    try {
      const built = buildIsolatedFormalHost(directory);
      const settingsBefore = runningSettingsPids();
      host = startHost(built.executable, built.runtimeDirectory);
      configure(host);
      await host.waitFor((event) => event.type === "ready");

      const initial = await waitForGuide(built.probe, host.child.pid!);
      expect(initial.buttonEnabledKnown).toBe(true);
      expect(initial.buttonEnabled).toBe(true);
      expect(initial.appActive).toBe(true);
      expect(initial.windowMainKnown).toBe(true);
      expect(initial.windowMain).toBe(true);

      const moveOut = probe(built.probe, host.child.pid!, "move-out");
      expect(moveOut.posted).toBe(true);
      await delay(180);
      const normal = probe(built.probe, host.child.pid!, "inspect", join(snapshotDirectory, "normal.png"));
      expect(normal.buttonEnabled).toBe(true);
      expect(normal.appActive).toBe(true);
      expect(normal.windowMain).toBe(true);

      const moveIn = probe(built.probe, host.child.pid!, "move-button");
      expect(moveIn.posted).toBe(true);
      await delay(250);
      const hover = probe(built.probe, host.child.pid!, "inspect", join(snapshotDirectory, "hover.png"));

      const down = probe(built.probe, host.child.pid!, "down");
      expect(down.posted).toBe(true);
      pointerHeld = true;
      await delay(220);
      const held = probe(built.probe, host.child.pid!, "inspect", join(snapshotDirectory, "mouse-down.png"));

      for (let attempt = 0; attempt < 6; attempt++) {
        const dragOut = probe(built.probe, host.child.pid!, "drag-out");
        expect(dragOut.posted).toBe(true);
      }
      await delay(220);
      const dragged = probe(built.probe, host.child.pid!, "inspect", join(snapshotDirectory, "drag-out-held.png"));

      const release = probe(built.probe, host.child.pid!, "up");
      expect(release.posted).toBe(true);
      pointerHeld = false;
      await delay(350);
      const cancelled = probe(built.probe, host.child.pid!, "inspect", join(snapshotDirectory, "cancelled.png"));
      expect(cancelled.foundWindow).toBe(true);
      expect(cancelled.foundButton).toBe(true);
      expect(cancelled.buttonEnabled).toBe(true);
      expect(cancelled.appActive).toBe(true);
      expect(cancelled.windowMain).toBe(true);
      expect(host.allEvents.filter((event) => event.type === "allow" || event.type === "retry")).toHaveLength(0);
      const settingsAfterCancel = runningSettingsPids();
      expect(settingsAfterCancel).toEqual(settingsBefore);
      const cancelAllowOrRetryEvents = host.allEvents.filter((event) => event.type === "allow" || event.type === "retry").length;
      const postCancelMove = probe(built.probe, host.child.pid!, "move-out");
      expect(postCancelMove.posted).toBe(true);
      await delay(250);
      const cancelledOutside = probe(built.probe, host.child.pid!, "inspect", join(snapshotDirectory, "cancelled-outside.png"));
      expect(cancelledOutside.buttonEnabled).toBe(true);

      const rehover = probe(built.probe, host.child.pid!, "move-button");
      expect(rehover.posted).toBe(true);
      await delay(180);
      const recoveryHover = probe(built.probe, host.child.pid!, "inspect", join(snapshotDirectory, "recovery-hover.png"));
      const recoveryDown = probe(built.probe, host.child.pid!, "down");
      expect(recoveryDown.posted).toBe(true);
      pointerHeld = true;
      await delay(100);
      const recoveryUp = probe(built.probe, host.child.pid!, "up-button");
      expect(recoveryUp.posted).toBe(true);
      pointerHeld = false;
      await delay(650);
      const afterRecovery = probe(built.probe, host.child.pid!, "inspect", join(snapshotDirectory, "recovery-click.png"));
      expect(host.allEvents.filter((event) => event.type === "allow")).toHaveLength(1);
      expect(host.allEvents.filter((event) => event.type === "retry")).toHaveLength(0);
      const settingsAfterRecovery = runningSettingsPids();
      expect(settingsAfterRecovery).toEqual(settingsBefore);
      expect(afterRecovery.foundButton).toBe(true);
      expect(afterRecovery.buttonEnabled).toBe(true);
      expect(afterRecovery.appActive).toBe(true);

      const readings = { normal, hover, held, dragged, cancelled, cancelledOutside, recoveryHover, afterRecovery };
      const pixelCaptureComplete = Object.values(readings).every((reading) => reading.pixelKnown === true);
      if (pixelCaptureComplete) {
        // The installed original has no visual Allow hover treatment.
        expect(Math.abs(luminance(hover) - luminance(normal))).toBeLessThan(0.005);
        expect(luminance(held)).toBeLessThan(luminance(hover) - 0.005);
        expect(luminance(cancelled)).toBeGreaterThan(luminance(held) + 0.005);
        expect(Math.abs(luminance(cancelledOutside) - luminance(normal))).toBeLessThan(0.005);
        expect(Math.abs(luminance(recoveryHover) - luminance(normal))).toBeLessThan(0.005);
        expect(Math.abs(luminance(afterRecovery) - luminance(normal))).toBeLessThan(0.005);
      }

      console.log(`C14_ALLOW_STATE ${JSON.stringify({
        appearance: "Aqua/Light", copy: "English", active: normal.appActive, mainWindow: normal.windowMain,
        pixelCaptureComplete,
        readings: Object.fromEntries(Object.entries(readings).map(([name, reading]) => [name, {
          pixelKnown: reading.pixelKnown ?? false, rgb: reading.rgb ?? null, snapshot: reading.snapshot ?? null,
          buttonEnabled: reading.buttonEnabled, appActive: reading.appActive, windowMain: reading.windowMain,
        }])),
        cancelAllowOrRetryEvents,
        recoveryAllowCount: host.allEvents.filter((event) => event.type === "allow").length,
        settingsPidsStable: JSON.stringify(settingsBefore) === JSON.stringify(settingsAfterRecovery),
        recoveryButtonInteractive: afterRecovery.buttonEnabled,
        snapshots: snapshotDirectory,
        snapshotsRetained: Boolean(evidenceRoot),
      })}`);

      host.send({ type: "close" });
      const exit = await new Promise<{ code: number | null; signal: NodeJS.Signals | null }>((resolve) =>
        host!.child.once("close", (code, signal) => resolve({ code, signal })));
      expect(exit).toEqual({ code: 0, signal: null });
      host = undefined;
    } finally {
      if (pointerHeld && host && host.child.exitCode === null && host.child.signalCode === null) {
        try { probe(join(directory, "runtime", "permission-allow-state-c14"), host.child.pid!, "up"); } catch {}
      }
      if (host && host.child.exitCode === null && host.child.signalCode === null) {
        host.send({ type: "close" });
        await delay(100);
        if (host.child.exitCode === null) host.child.kill("SIGTERM");
      }
      rmSync(directory, { recursive: true, force: true });
      if (!evidenceRoot) rmSync(snapshotDirectory, { recursive: true, force: true });
    }
  },
  180_000,
);
