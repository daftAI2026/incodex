import { afterAll, beforeAll, expect, setDefaultTimeout, test } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";

const repositoryRoot = join(import.meta.dir, "..");
const nativeRoot = join(repositoryRoot, "native", "macos");
const runVisibleDiagnostic = process.env.INCODEX_RUN_PERMISSION_AX_KEYBOARD_DIAGNOSTIC === "1";

setDefaultTimeout(90_000);

let diagnostic: { appBundle: string; executable: string; directory: string } | undefined;

beforeAll(() => {
  if (process.platform === "darwin") diagnostic = buildDiagnostic();
});

afterAll(() => {
  if (diagnostic) rmSync(diagnostic.directory, { recursive: true, force: true });
});

test.skipIf(process.platform !== "darwin")(
  "compiles the opt-in current-host AX keyboard diagnostic without opening windows",
  () => {
    expect(diagnostic?.executable).toBeTruthy();
  },
);

test.skipIf(process.platform !== "darwin" || !runVisibleDiagnostic)(
  "prints current-host AX tree and guarded keyboard focus probes",
  () => {
    const outputFile = join(diagnostic!.directory, "diagnostic-output.jsonl");
    writeFileSync(outputFile, "", "utf8");
    const result = spawnSync("/usr/bin/open", [
      "-n",
      "-W",
      diagnostic!.appBundle,
      "--args",
      `--diagnostic-output=${outputFile}`,
    ], {
      cwd: repositoryRoot,
      encoding: "utf8",
      timeout: 75_000,
    });
    const diagnosticOutput = readFileSync(outputFile, "utf8");
    const output = `${result.stdout ?? ""}${result.stderr ?? ""}${diagnosticOutput}`;
    for (const line of output.split("\n")) {
      if (!line.startsWith("{")) continue;
      const event = JSON.parse(line) as { kind?: string };
      if (["FOCUS_SNAPSHOT", "KEY_PROBE", "STATE_TRANSITION", "DIAGNOSTIC_LIMITS", "CLEANUP"].includes(event.kind ?? "")) {
        console.log(line);
      }
    }
    expect(result.status, output || String(result.error ?? "AX keyboard diagnostic failed")).toBe(0);
    expect(output).toContain('"kind":"AX_TREE_BEGIN"');
    expect(output).toContain('"state":"initial"');
    expect(output).toContain('"state":"helper-prohibited-placeholder"');
    expect(output).toContain('"state":"after-back"');
    expect(output).toContain('"kind":"DIAGNOSTIC_COMPLETED"');
    expect(output).toContain('"voiceOver":"not toggled or tested"');
    expect(output).toContain('"referenceParity":"not asserted"');
    const initialWindowTitles = diagnosticOutput.split("\n")
      .filter((line) => line.startsWith("{"))
      .map((line) => JSON.parse(line) as { kind?: string; state?: string; role?: string; title?: string })
      .filter((event) => event.kind === "AX_NODE" && event.state === "initial" && event.role === "AXWindow")
      .map((event) => event.title);
    expect(initialWindowTitles).toContain("Enable ChatGPT scripting");
  },
  80_000,
);

function buildDiagnostic(): { appBundle: string; executable: string; directory: string } {
  const directory = mkdtempSync(join(tmpdir(), "incodex-permission-ax-keyboard-"));
  const executableName = "permission-ax-keyboard-diagnostic";
  const appBundle = join(directory, "PermissionAXKeyboardDiagnostic.app");
  const contents = join(appBundle, "Contents");
  const executable = join(contents, "MacOS", executableName);
  const bundleIdentifier = `com.incodex.test.permission-ax-keyboard.${randomUUID()}`;
  mkdirSync(join(contents, "MacOS"), { recursive: true });
  writeFileSync(join(contents, "Info.plist"), [
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
    "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">",
    "<plist version=\"1.0\"><dict>",
    "<key>CFBundleDevelopmentRegion</key><string>en</string>",
    `<key>CFBundleExecutable</key><string>${executableName}</string>`,
    `<key>CFBundleIdentifier</key><string>${bundleIdentifier}</string>`,
    "<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>",
    "<key>CFBundleName</key><string>Permission AX Keyboard Diagnostic</string>",
    "<key>CFBundlePackageType</key><string>APPL</string>",
    "<key>CFBundleShortVersionString</key><string>1</string>",
    "<key>CFBundleVersion</key><string>1</string>",
    "<key>LSMinimumSystemVersion</key><string>12.0</string>",
    "<key>NSPrincipalClass</key><string>NSApplication</string>",
    "</dict></plist>",
  ].join("\n"), "utf8");
  const architecture = process.arch === "arm64" ? "arm64" : "x86_64";
  const result = spawnSync("xcrun", [
    "swiftc",
    "-parse-as-library",
    "-module-name",
    "IncodexPermissionAXKeyboardDiagnostic",
    "-target",
    `${architecture}-apple-macos12.0`,
    join(nativeRoot, "permission-views.swift"),
    join(nativeRoot, "permission-host-settings.swift"),
    join(nativeRoot, "permission-host-flight.swift"),
    join(nativeRoot, "permission-host-presenter.swift"),
    join(import.meta.dir, "native", "permission-ax-keyboard-diagnostic.swift"),
    "-framework",
    "ApplicationServices",
    "-framework",
    "CoreGraphics",
    "-o",
    executable,
  ], {
    cwd: repositoryRoot,
    encoding: "utf8",
    timeout: 75_000,
  });
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;
  expect(result.status, output || String(result.error ?? "AX keyboard diagnostic failed to compile")).toBe(0);
  return { appBundle, executable, directory };
}
