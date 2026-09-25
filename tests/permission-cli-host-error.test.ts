import { expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createHash } from "node:crypto";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { ACCESSIBILITY_SETUP_COPY } from "../src/runtime/incognito-copy.ts";
import { sharedPermissionCopy } from "../src/permission-shared-copy.ts";

const root = join(import.meta.dir, "..");
const nativeRoot = join(root, "native", "macos");
const runVisible = process.env.INCODEX_RUN_G10_FORMAL_HOST_ERROR === "1";
const compileG10Host = process.env.INCODEX_COMPILE_G10_HOST === "1";
const english = sharedPermissionCopy(ACCESSIBILITY_SETUP_COPY).en!;
const delay = (milliseconds: number) => new Promise((resolvePromise) => setTimeout(resolvePromise, milliseconds));
const sha256 = (value: Buffer | string) => createHash("sha256").update(value).digest("hex");

function record(outputDirectory: string, entry: Record<string, unknown>): void {
  const path = join(outputDirectory, "events.jsonl");
  const body = `${JSON.stringify({ at: new Date().toISOString(), ...entry })}\n`;
  writeFileSync(path, body, { mode: 0o600, flag: existsSync(path) ? "a" : "wx" });
}

function privateOutputDirectory(): string {
  const raw = process.env.INCODEX_G10_OUT;
  if (!raw || !isAbsolute(raw)) throw new Error("visible G10 diagnostic requires an absolute INCODEX_G10_OUT that does not exist yet");
  const output = resolve(raw);
  if (existsSync(output)) throw new Error(`refusing to reuse existing G10 evidence directory: ${output}`);
  const parent = dirname(output);
  if (realpathSync(parent) !== parent) throw new Error(`G10 output parent must be canonical (no symlink components): ${parent}`);
  mkdirSync(output, { mode: 0o700 });
  chmodSync(output, 0o700);
  return output;
}

function isolatedHostSource(): string {
  let source = readFileSync(join(nativeRoot, "permission-host.swift"), "utf8");
  const gate = /static func canPresentOfficialTarget\(\) -> Bool \{[\s\S]*?\n {4}\}/;
  const matchedGate = source.match(gate)?.[0];
  if (!matchedGate?.includes("officialCodexBundleIdentifier")) throw new Error("formal host presentation gate seam was not found");
  source = source.replace(gate, "static func canPresentOfficialTarget() -> Bool { true\n    }");
  const localeGate = /let targetBundlePath: String\n#if INCODEX_PERMISSION_HOST_TESTING\n[\s\S]*?\n#endif/;
  if (!localeGate.test(source)) throw new Error("formal host target-bundle locale seam was not found");
  return source.replace(
    localeGate,
    'let targetBundlePath = ProcessInfo.processInfo.environment["INCODEX_PERMISSION_HOST_TARGET_BUNDLE"] ?? officialCodexAppPath',
  );
}

function buildFormalHost(workDirectory: string): { executable: string; axProbe: string; axTrustProbe: string; bundle: string; sourceHashes: Record<string, string> } {
  const bundle = join(workDirectory, "CodexLocaleFixture.app");
  const resources = join(bundle, "Contents", "Resources");
  mkdirSync(join(resources, "en.lproj"), { recursive: true, mode: 0o700 });
  writeFileSync(join(bundle, "Contents", "Info.plist"), `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.openai.codex</string>
<key>CFBundleDevelopmentRegion</key><string>en</string>
<key>CFBundleExecutable</key><string>ChatGPT</string>
</dict></plist>`);

  const hostSourcePath = join(workDirectory, "permission-host-g10-fixture.swift");
  writeFileSync(hostSourcePath, isolatedHostSource(), { mode: 0o600 });
  const executable = join(workDirectory, "incodex-permission-host-g10-fixture");
  const sdk = spawnSync("xcrun", ["--sdk", "macosx", "--show-sdk-path"], { encoding: "utf8", timeout: 10_000 });
  expect(sdk.status, sdk.stderr).toBe(0);
  const sources = [
    "permission-views.swift",
    "permission-host-settings.swift",
    "permission-host-flight.swift",
    "permission-host-presenter.swift",
  ].map((name) => join(nativeRoot, name));
  const architecture = process.arch === "arm64" ? "arm64" : "x86_64";
  const build = spawnSync("xcrun", ["swiftc", "-parse-as-library", "-emit-executable",
    "-module-name", "IncodexG10FormalCliHostDiagnostic", "-disable-autolinking-runtime-compatibility",
    "-target", `${architecture}-apple-macos12.0`, "-sdk", sdk.stdout.trim(),
    ...sources, hostSourcePath, "-Xlinker", "-export_dynamic", "-o", executable],
  { cwd: root, encoding: "utf8", timeout: 90_000 });
  const buildOutput = `${build.stdout ?? ""}${build.stderr ?? ""}`;
  expect(build.status, buildOutput || String(build.error ?? "formal Swift child host did not compile")).toBe(0);
  chmodSync(executable, 0o700);

  const axProbe = join(workDirectory, "permission-cli-host-error-ax-probe");
  const axBuild = spawnSync("clang", ["-fobjc-arc", "-framework", "Cocoa", "-framework", "ApplicationServices",
    "-framework", "CoreGraphics", join(import.meta.dir, "native", "permission-runtime-error-ax-smoke.m"), "-o", axProbe],
  { cwd: root, encoding: "utf8", timeout: 60_000 });
  expect(axBuild.status, axBuild.stderr).toBe(0);
  chmodSync(axProbe, 0o700);
  const trustSource = join(workDirectory, "ax-trust-preflight.m");
  writeFileSync(trustSource, "#import <ApplicationServices/ApplicationServices.h>\nint main(void) { return AXIsProcessTrusted() ? 0 : 2; }\n", { mode: 0o600 });
  const axTrustProbe = join(workDirectory, "permission-cli-host-error-ax-trust");
  const trustBuild = spawnSync("clang", ["-framework", "ApplicationServices", trustSource, "-o", axTrustProbe], {
    cwd: root, encoding: "utf8", timeout: 60_000,
  });
  expect(trustBuild.status, trustBuild.stderr).toBe(0);
  chmodSync(axTrustProbe, 0o700);
  return {
    executable,
    axProbe,
    axTrustProbe,
    bundle,
    sourceHashes: Object.fromEntries([...sources, join(nativeRoot, "permission-host.swift"), join(nativeRoot, "permission-views.swift")]
      .map((path) => [path.replace(`${root}/`, ""), sha256(readFileSync(path))])),
  };
}

function inspect(axProbe: string, pid: number, title: string, body = "", button = "", press = false): Record<string, any> {
  const result = spawnSync(axProbe, [String(pid), title, body, button, ...(press ? ["press"] : [])], {
    encoding: "utf8", timeout: 10_000,
  });
  expect(result.status, result.stderr).toBe(0);
  return JSON.parse(result.stdout.trim());
}

async function waitForAX(axProbe: string, pid: number, title: string, body = "", button = "", timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  let last: Record<string, any> | undefined;
  while (Date.now() < deadline) {
    last = inspect(axProbe, pid, title, body, button);
    if (last.foundWindow && (!body || last.foundBody) && (!button || last.foundButton)) return last;
    await delay(100);
  }
  throw new Error(`native host AX content did not appear: ${JSON.stringify(last)}`);
}

function exactProcessCommand(pid: number): string {
  const result = spawnSync("/bin/ps", ["-p", String(pid), "-o", "command="], { encoding: "utf8", timeout: 5_000 });
  return result.status === 0 ? result.stdout.trim() : "";
}

async function waitForHostPid(pidFile: string, executable: string, cargo: ChildProcessWithoutNullStreams, output: () => string): Promise<number> {
  const deadline = Date.now() + 45_000;
  while (Date.now() < deadline) {
    if (existsSync(pidFile)) {
      const pid = Number(readFileSync(pidFile, "utf8").trim());
      expect(Number.isSafeInteger(pid) && pid > 0).toBe(true);
      expect(exactProcessCommand(pid)).toContain(executable);
      return pid;
    }
    if (cargo.exitCode !== null) throw new Error(`Rust diagnostic exited before publishing child PID: ${output()}`);
    await delay(50);
  }
  throw new Error(`Rust diagnostic did not start the native child host: ${output()}`);
}

function cargoResult(child: ChildProcessWithoutNullStreams, output: () => string, timeoutMs = 40_000): Promise<{ code: number | null; signal: NodeJS.Signals | null }> {
  return new Promise((resolvePromise, rejectPromise) => {
    const timer = setTimeout(() => rejectPromise(new Error(`Rust visible diagnostic did not finish after dismissal: ${output()}`)), timeoutMs);
    child.once("close", (code, signal) => {
      clearTimeout(timer);
      resolvePromise({ code, signal });
    });
  });
}

test("G10 failure UI can run only behind explicit opt-in and a new private output directory", () => {
  const source = readFileSync(join(import.meta.dir, "permission-cli-host-error.test.ts"), "utf8");
  expect(source).toContain("INCODEX_RUN_G10_FORMAL_HOST_ERROR === \"1\"");
  expect(source).toContain("INCODEX_COMPILE_G10_HOST === \"1\"");
  expect(source).toContain("absolute INCODEX_G10_OUT that does not exist yet");
  expect(source).toContain("G10_DIAGNOSTIC_RESET_FAILURE");
  expect(source).toContain("settingsCalls: 0");
  expect(source).toContain("test.skipIf(process.platform !== \"darwin\" || !runVisible)");
});

test.skipIf(process.platform !== "darwin" || !compileG10Host)("G10 protocol fixture compiles the real Swift host and AX probe without launching either", () => {
  const workDirectory = mkdtempSync(join(tmpdir(), "incodex-g10-formal-host-build-"));
  try {
    const host = buildFormalHost(workDirectory);
    expect(existsSync(host.executable)).toBe(true);
    expect(existsSync(host.axProbe)).toBe(true);
    expect(existsSync(host.axTrustProbe)).toBe(true);
  } finally {
    rmSync(workDirectory, { recursive: true, force: true });
  }
}, 120_000);

test.skipIf(process.platform !== "darwin" || !runVisible)("Rust coordinator and production child-host protocol show and dismiss a mocked reset-failure page", async () => {
  const outputDirectory = privateOutputDirectory();
  const workDirectory = mkdtempSync(join(tmpdir(), "incodex-g10-formal-host-error-"));
  const sourceHome = join(workDirectory, "source-home");
  mkdirSync(sourceHome, { mode: 0o700 });
  writeFileSync(join(sourceHome, "config.toml"), 'localeOverride = "en"\n', { mode: 0o600 });
  const pidFile = join(workDirectory, "child-host.pid");
  const testPidFile = join(workDirectory, "rust-test.pid");
  let cargo: ChildProcessWithoutNullStreams | undefined;
  let hostPID: number | undefined;
  let capturedOutput = "";
  let finalStatus: { code: number | null; signal: NodeJS.Signals | null } | undefined;
  let host: ReturnType<typeof buildFormalHost> | undefined;
  let runFailure: string | undefined;
  const append = (entry: Record<string, unknown>) => record(outputDirectory, entry);

  try {
    host = buildFormalHost(workDirectory);
    const axTrusted = spawnSync(host.axTrustProbe, [], { encoding: "utf8", timeout: 5_000 });
    if (axTrusted.status !== 0) throw new Error("Accessibility is not already trusted for the diagnostic probe; refused before starting any UI and did not request permission");
    const cargoArguments = ["test", "-p", "incodex-cli", "--lib",
      "accessibility_guide_host::tests::rust_coordinator_process_host_displays_mocked_reset_failure_until_dismissed",
      "--", "--ignored", "--exact", "--nocapture"];
    const environment: NodeJS.ProcessEnv = {
      ...process.env,
      INCODEX_G10_HOST_EXECUTABLE: host.executable,
      INCODEX_G10_HOST_PID_FILE: pidFile,
      INCODEX_G10_TEST_PID_FILE: testPidFile,
      INCODEX_PERMISSION_HOST_TARGET_BUNDLE: host.bundle,
      INCODEX_SOURCE_HOME: sourceHome,
    };
    delete environment.INCODEX_PERMISSION_HOST_DISABLE_PRESENTATION;
    cargo = spawn("cargo", cargoArguments, { cwd: root, env: environment, stdio: ["pipe", "pipe", "pipe"] });
    cargo.stdin.end();
    cargo.stdout.setEncoding("utf8");
    cargo.stderr.setEncoding("utf8");
    cargo.stdout.on("data", (chunk: string) => { capturedOutput += chunk; append({ type: "rust-stdout", text: chunk }); });
    cargo.stderr.on("data", (chunk: string) => { capturedOutput += chunk; append({ type: "rust-stderr", text: chunk }); });
    append({
      type: "start",
      command: "cargo test -p incodex-cli --lib accessibility_guide_host::tests::rust_coordinator_process_host_displays_mocked_reset_failure_until_dismissed -- --ignored --exact --nocapture",
      repoHead: spawnSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).stdout.trim(),
      sourceHashes: host.sourceHashes,
      mockedOnly: "GuideOps::reset returns G10_DIAGNOSTIC_RESET_FAILURE; launch/probe/window/settings/TCC operations do not call the OS",
      settingsOpened: false,
      tccChanged: false,
      axTrustPreflightPassed: true,
    });
    hostPID = await waitForHostPid(pidFile, host.executable, cargo, () => capturedOutput);
    append({ type: "child-host-started", hostPID, executable: host.executable, transport: "production Rust ProcessGuideHost private stdin/stdout, nonce-bound" });

    const initial = await waitForAX(host.axProbe, hostPID, english.title, "", english.repair);
    expect(initial.buttonEnabledKnown).toBe(true);
    expect(initial.buttonEnabled).toBe(true);
    expect(initial.cgOnscreenWindowCount).toBeGreaterThan(0);
    append({ type: "initial-page-visible", result: initial });

    const allow = inspect(host.axProbe, hostPID, english.title, "", english.repair, true);
    expect(allow.pressResult).toBe(0);
    append({ type: "allow-pressed-on-test-host", result: allow, note: "this reaches only mocked GuideOps::reset; no system permission API is called" });

    const error = await waitForAX(host.axProbe, hostPID, english.errorTitle, english.errorBody, english.later);
    await delay(500);
    const stableError = inspect(host.axProbe, hostPID, english.errorTitle, english.errorBody, english.later);
    const disabledAllow = inspect(host.axProbe, hostPID, english.errorTitle, english.errorBody, english.repair);
    expect(error.foundBody).toBe(true);
    expect(error.cgOnscreenWindowCount).toBeGreaterThan(0);
    expect(stableError.cgOnscreenWindowCount).toBe(error.cgOnscreenWindowCount);
    expect(stableError.axWindowCount).toBe(error.axWindowCount);
    expect(disabledAllow.buttonEnabledKnown).toBe(true);
    expect(disabledAllow.buttonEnabled).toBe(false);
    append({ type: "failure-page-visible-and-stable", result: stableError, allowButton: disabledAllow });

    const skip = inspect(host.axProbe, hostPID, english.errorTitle, english.errorBody, english.later, true);
    expect(skip.pressResult).toBe(0);
    append({ type: "error-page-dismissed-by-skip", result: skip });
    finalStatus = await cargoResult(cargo, () => capturedOutput);
    expect(finalStatus).toEqual({ code: 0, signal: null });
    expect(capturedOutput).toContain("test accessibility_guide_host::tests::rust_coordinator_process_host_displays_mocked_reset_failure_until_dismissed ... ok");
    append({ type: "coordinator-returned", status: finalStatus, rustTestPassed: true, resetCalls: 1, settingsCalls: 0 });
    writeFileSync(join(outputDirectory, "result.json"), `${JSON.stringify({
      kind: "g10-rust-coordinator-child-host-error-diagnostic",
      status: "passed-diagnostic-only",
      repoHead: spawnSync("git", ["rev-parse", "HEAD"], { cwd: root, encoding: "utf8" }).stdout.trim(),
      hostPID,
      hostSourceSha256: host.sourceHashes,
      failureSource: "injected GuideOps::reset error; no tccutil/system permissions/Settings/ChatGPT operation",
      coverage: ["Rust permission coordinator", "nonce-bound ProcessGuideHost protocol", "production Swift host protocol", "production SwiftUI/AppKit presenter error page", "AX-visible error copy", "disabled Allow", "Skip dismissal"],
      limitation: "test-only host executable bypasses the official foreground-app gate and Rust test-only factory bypasses installed Runtime/code-signature lookup; this is not a real reset failure, installed CLI entry/argument run, or a real System Settings-in-presence flow; Settings was not launched or foregrounded, and evidence is AX/CG on-screen state rather than a screenshot",
      initial,
      error: stableError,
      disabledAllow,
      skip,
    }, null, 2)}\n`, { mode: 0o600, flag: "wx" });
  } catch (error) {
    runFailure = error instanceof Error ? error.stack ?? error.message : String(error);
    throw error;
  } finally {
    if (cargo && cargo.exitCode === null && cargo.signalCode === null) {
      if (hostPID && host) {
        for (const [title, body, button] of [[english.errorTitle, english.errorBody, english.later], [english.title, "", english.later]]) {
          try {
            if (exactProcessCommand(hostPID).includes(host.executable)) {
              const dismissal = inspect(host.axProbe, hostPID, title, body, button, true);
              if (dismissal.pressResult === 0) break;
            }
          } catch { /* cleanup is best-effort; the Rust diagnostic itself has a bounded 30-second choice timeout */ }
        }
      }
      try { await cargoResult(cargo, () => capturedOutput, 35_000); } catch {
        if (existsSync(testPidFile)) {
          const testPID = Number(readFileSync(testPidFile, "utf8").trim());
          const command = exactProcessCommand(testPID);
          if (Number.isSafeInteger(testPID) && command.includes("incodex_cli-") && command.includes("rust_coordinator_process_host_displays_mocked_reset_failure_until_dismissed")) {
            process.kill(testPID, "SIGTERM");
          }
        }
        try { await cargoResult(cargo, () => capturedOutput, 10_000); } catch { cargo.kill("SIGTERM"); }
        if (hostPID && host && exactProcessCommand(hostPID).includes(host.executable)) process.kill(hostPID, "SIGTERM");
      }
    }
    if (host) rmSync(workDirectory, { recursive: true, force: true });
    if (!existsSync(join(outputDirectory, "result.json"))) {
      writeFileSync(join(outputDirectory, "result.json"), `${JSON.stringify({
        kind: "g10-rust-coordinator-child-host-error-diagnostic",
        status: "failed-diagnostic-only",
        hostPID: hostPID ?? null,
        failure: runFailure ?? "diagnostic did not reach evidence finalization",
        rustOutput: capturedOutput,
      }, null, 2)}\n`, { mode: 0o600, flag: "wx" });
    }
  }
});
