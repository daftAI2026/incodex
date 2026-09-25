import { expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";

const script = join(import.meta.dir, "permission-swift-candidate-visual.ts");
const helper = join(import.meta.dir, "permission-swift-candidate-visual-helper.swift");

test("visual candidate runner refuses to launch without explicit visible-UI opt-in", () => {
  const result = spawnSync("bun", [script], { encoding: "utf8", timeout: 10_000 });
  expect(result.status).toBe(2);
  expect(result.stdout + result.stderr).toContain("--run --acknowledge-visible-ui");
  expect(result.stdout + result.stderr).not.toContain("Launching official ChatGPT");
});

test("visible-UI opt-in still requires a fresh, explicit output directory", () => {
  const parent = mkdtempSync(join(tmpdir(), "incodex-visual-safety-test-"));
  const output = join(parent, "evidence");
  try {
    const result = spawnSync("bun", [script, "--run", "--out", output], { encoding: "utf8", timeout: 10_000 });
    expect(result.status).toBe(2);
    expect(result.stdout + result.stderr).toContain("--acknowledge-visible-ui");
    expect(() => statSync(output)).toThrow();
  } finally {
    rmSync(parent, { recursive: true, force: true });
  }
});

test("placeholder AXPress opt-in cannot bypass the visible-run gates", () => {
  const result = spawnSync("bun", [script, "--diagnose-placeholder-axpress"], { encoding: "utf8", timeout: 10_000 });
  expect(result.status).toBe(2);
  expect(result.stdout + result.stderr).toContain("--run --acknowledge-visible-ui");
  expect(result.stdout + result.stderr).not.toContain("Opt-in accepted");
});

test("initial keyboard Allow diagnostic cannot bypass the visible-run gates", () => {
  const result = spawnSync("bun", [script, "--diagnose-initial-keyboard"], { encoding: "utf8", timeout: 10_000 });
  expect(result.status).toBe(2);
  expect(result.stdout + result.stderr).toContain("--run --acknowledge-visible-ui");
  expect(result.stdout + result.stderr).not.toContain("Opt-in accepted");
});

test("runner source is read-only with respect to TCC and compiles the production gate", () => {
  const source = readFileSync(script, "utf8");
  const helperSource = readFileSync(helper, "utf8");
  expect(helperSource).toContain("CGPreflightScreenCaptureAccess");
  expect(helperSource).toContain('"postEventAccessAlreadyGranted": CGPreflightPostEventAccess()');
  expect(helperSource).toContain("AXIsProcessTrusted()");
  expect(helperSource).toContain("#available(macOS 14.0, *)");
  expect(helperSource).not.toContain("CGRequestScreenCaptureAccess");
  expect(source + helperSource).not.toContain("tccutil reset");
  expect(source).not.toMatch(/tccutil\s+reset/i);
  const compile = source.split("function compileCandidate")[1]?.split("function sha256")[0] ?? "";
  expect(compile).toContain("...hostSources");
  expect(compile).not.toContain("-D");
  expect(compile).not.toContain("INCODEX_PERMISSION_HOST_STUB");
  expect(compile).not.toContain("INCODEX_PERMISSION_HOST_TESTING");
  expect(helperSource).toContain("AXWindow");
  expect(helperSource).toContain("kAXHiddenAttribute");
  expect(helperSource).toContain("frontmostWindowBeforePress");
  expect(helperSource).toContain("ownerCGWindow");
  expect(helperSource).toContain("higher z-order window");
  for (const name of [
    "permission-views.swift",
    "permission-host-settings.swift",
    "permission-host-flight.swift",
    "permission-host-presenter.swift",
    "permission-host.swift",
  ]) expect(source).toContain(name);
});

test("unsupported AXHidden does not hide an otherwise geometrically visible button", () => {
  const helper = readFileSync(join(import.meta.dir, "permission-swift-candidate-visual-helper.swift"), "utf8");
  expect(helper).toContain("as? Bool == true");
  expect(helper).toContain("targetWindows.count == 1");
  expect(helper).toContain("occluders.isEmpty");
  expect(helper).not.toContain("as? Bool == false else");
});

test("remote overlay exception requires an explicit opt-in and exact window identity", () => {
  const helper = readFileSync(join(import.meta.dir, "permission-swift-candidate-visual-helper.swift"), "utf8");
  expect(helper).toContain("INCODEX_VISUAL_IGNORE_UU_REMOTE_OVERLAY");
  expect(helper).toContain('ownerName == "UURemoteServer"');
  expect(helper).toContain("layer == 2147483631");
  expect(helper).toContain('frontmost?.bundleIdentifier == "com.apple.systempreferences"');
  expect(helper).toContain('(targetWindow["layer"] as? Int) == 3');
  expect(helper).toContain('"ignoredRemoteOverlays": ignoredRemoteOverlays');
});

test("press output reports the matched button AX actions and label metadata", () => {
  const helper = readFileSync(join(import.meta.dir, "permission-swift-candidate-visual-helper.swift"), "utf8");
  expect(helper).toContain("AXUIElementCopyActionNames(button, &actionNames)");
  expect(helper).toContain('"axActionNames": [String](actionNames as? [String] ?? [])');
  expect(helper).toContain('"buttonAXTitle": buttonTitle as Any? ?? NSNull()');
  expect(helper).toContain('"buttonAXDescription": buttonDescription as Any? ?? NSNull()');
});

test("placeholder AXPress is separately opted in inside the existing helper-to-Back flow", () => {
  const source = readFileSync(script, "utf8");
  const helperSource = readFileSync(helper, "utf8");
  expect(source).toContain("diagnosePlaceholderAXPress: false");
  expect(source).toContain("--diagnose-placeholder-axpress");
  const helperLanded = source.indexOf('"helper-landed-window-order"');
  const placeholderPress = source.indexOf('"press-settings-placeholder"');
  const backPress = source.indexOf('const backPress = helperCall');
  expect(helperLanded).toBeGreaterThanOrEqual(0);
  expect(placeholderPress).toBeGreaterThan(helperLanded);
  expect(backPress).toBeGreaterThan(placeholderPress);
  expect(source).toContain('selected.copy.completeInSettings');
  expect(source).toContain("retryAfter !== retryBefore + 1");
  expect(source).toContain("allowAfter !== allowBefore");
  expect(helperSource).toContain('case "press-settings-placeholder":');
  expect(helperSource).toContain('"buttonAXRole": buttonRole as Any? ?? NSNull()');
  expect(helperSource).toContain('guard buttonRole == (kAXButtonRole as String)');
  expect(helperSource).toContain('guard actionNameList.contains(kAXPressAction as String)');
  expect(helperSource).toContain('frontmost?.bundleIdentifier == "com.apple.systempreferences"');
  expect(helperSource).toContain('(targetWindow["layer"] as? Int) == 0');
  expect(helperSource).toContain('guard !settingsOccluders.isEmpty');
  expect(helperSource).toContain('expectedPlaceholderFocus');
  expect(helperSource).toContain('(expectedHostFocus || expectedHelperFocus || expectedPlaceholderFocus)');
  expect(helperSource).toContain('guard occluders.isEmpty else');
});

test("initial keyboard diagnostic records Allow and Skip focus, then activates Allow exactly once with Return", () => {
  const source = readFileSync(script, "utf8");
  const helperSource = readFileSync(helper, "utf8");
  expect(source).toContain("diagnoseInitialKeyboard: false");
  expect(source).toContain("--diagnose-initial-keyboard");
  expect(source).toContain("initial-keyboard-allow");
  expect(source).toContain("assertInitialAllowKeyboardResult");
  expect(source).toContain("initialCountsBefore.allow !== 0 || initialCountsBefore.retry !== 0");
  expect(source).toContain("initialCountsAfter.allow !== 1 || initialCountsAfter.retry !== 0");
  const ready = source.indexOf('await takeScreenshot(binaries.helper, outputDirectory, "initial-ready", 1)');
  const keyboardAllow = source.indexOf('const keyboardAllow = helperCall(binaries.helper, [');
  const axAllow = source.indexOf('const allowPress = helperCall(binaries.helper, ["press"');
  expect(ready).toBeGreaterThanOrEqual(0);
  expect(keyboardAllow).toBeGreaterThan(ready);
  expect(axAllow).toBeGreaterThan(keyboardAllow);
  expect(source).toContain('const skipLabel = selected.copy.later ?? "Skip"');
  expect(helperSource).toContain('case "keyboard-allow":');
  expect(helperSource).toContain("frontmost?.processIdentifier == pid");
  expect(helperSource).toContain("CGPreflightPostEventAccess()");
  expect(source).toContain("preflight.postEventAccessAlreadyGranted !== true");
  expect(helperSource).toContain("kAXFocusedUIElementAttribute");
  expect(helperSource).toContain("buttonAXEnabled");
  expect(helperSource).toContain("buttonAXFocused");
  expect(helperSource).toContain("virtualKey: 36");
  expect(helperSource).toContain("down.post(tap: .cghidEventTap)");
  expect(helperSource).toContain("up.post(tap: .cghidEventTap)");
  expect(helperSource).not.toContain("CGRequestPostEventAccess");
  expect(source.indexOf("preflight.postEventAccessAlreadyGranted !== true")).toBeLessThan(source.indexOf("await waitForOfficialApp("));
});

test("Back and Settings placeholder presses report one enabled AXButton with AXPress", () => {
  const source = readFileSync(script, "utf8");
  const helperSource = readFileSync(helper, "utf8");
  expect(source).toContain("assertBackButtonResult(backPress, backTitle)");
  expect(source).toContain("assertPlaceholderButtonResult(placeholderPress, placeholderLabel)");
  expect(helperSource).toContain('"buttonAXEnabled": copyAttribute(button, kAXEnabledAttribute as CFString) as? Bool as Any? ?? NSNull()');
  expect(source).toContain("function assertBackButtonResult");
  expect(source).toContain('actions?.includes("AXPress")');
  expect(helperSource).toContain('"buttonAXRole": buttonRole as Any? ?? NSNull()');
  expect(helperSource).toContain('"buttonAXEnabled": copyAttribute(button, kAXEnabledAttribute as CFString) as? Bool as Any? ?? NSNull()');
  expect(helperSource).toContain('"axActionNames": [String](actionNames as? [String] ?? [])');
});

test("self-test mode is available without visible UI", () => {
  const result = spawnSync("bun", [script, "--self-test"], { encoding: "utf8", timeout: 10_000 });
  expect(result.status).toBe(0);
  expect(result.stdout).toContain("no UI or permission APIs were used");
});
