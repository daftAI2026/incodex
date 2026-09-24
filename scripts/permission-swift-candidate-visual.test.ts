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

test("runner source is read-only with respect to TCC and compiles the production gate", () => {
  const source = readFileSync(script, "utf8");
  const helperSource = readFileSync(helper, "utf8");
  expect(helperSource).toContain("CGPreflightScreenCaptureAccess");
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

test("self-test mode is available without visible UI", () => {
  const result = spawnSync("bun", [script, "--self-test"], { encoding: "utf8", timeout: 10_000 });
  expect(result.status).toBe(0);
  expect(result.stdout).toContain("no UI or permission APIs were used");
});
