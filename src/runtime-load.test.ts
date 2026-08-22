import { describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { targetStateDir } from "./runtime/incodex-instance.cts";
import { devHotEnabled, hotHomeRoot, resolveRuntimeFile } from "./runtime/incodex-runtime-load.cts";

describe("runtime load", () => {
  test("HOME missing does not yield a relative .incodex path", () => {
    expect(hotHomeRoot({})).toBeNull();
    expect(hotHomeRoot({ HOME: "" })).toBeNull();
    expect(hotHomeRoot({ HOME: "/Users/me" })).toBe("/Users/me/.incodex");
  });

  test("production ignores home overrides unless INCODEX_DEV_HOT=1", () => {
    const home = mkdtempSync(join(tmpdir(), "incodex-hot-"));
    const bundledDir = mkdtempSync(join(tmpdir(), "incodex-bundle-"));
    writeFileSync(join(bundledDir, "incodex-main.cjs"), "bundled");
    const dest = targetStateDir(join(home, ".incodex"), "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT");
    mkdirSync(dest, { recursive: true });
    writeFileSync(join(dest, "incodex-main.cjs"), "override");

    expect(devHotEnabled({})).toBe(false);
    expect(
      resolveRuntimeFile("incodex-main.cjs", bundledDir, { HOME: home }, "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT"),
    ).toBe(join(bundledDir, "incodex-main.cjs"));
    expect(
      resolveRuntimeFile(
        "incodex-main.cjs",
        bundledDir,
        { HOME: home, INCODEX_DEV_HOT: "1" },
        "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT",
      ),
    ).toBe(join(dest, "incodex-main.cjs"));
  });

  test("the asar loader fail-opens non-blocking attach errors", () => {
    const loader = readFileSync(join(import.meta.dir, "runtime/incodex-loader.cts"), "utf8");
    expect(loader).toContain("await loadMain();");
    expect(loader).toContain("require(originalMain())");
    expect(loader).toContain('error?.code === "INCODEX_STARTUP_BLOCKED"');
    expect(loader.indexOf("require(originalMain())")).toBeGreaterThan(loader.indexOf("await loadMain()"));
    expect(loader).not.toContain('require("./incodex-main.cjs")');
    expect(loader).toContain("current.json");
  });

  test("the loader gates official main on the incognito lease startup", () => {
    const loader = readFileSync(join(import.meta.dir, "runtime/incodex-loader.cts"), "utf8");
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    expect(loader).toContain("const runtime = require(file);");
    expect(loader).toContain("await runtime.startupGate");
    expect(loader).toContain('error?.code === "INCODEX_STARTUP_BLOCKED"');
    expect(loader.indexOf("require(originalMain())")).toBeGreaterThan(loader.indexOf("await loadMain()"));
    expect(main).toContain("const startupGate = attachElectron();");
    expect(main).toContain('error.code = "INCODEX_STARTUP_BLOCKED"');
  });

  test("an ordinary incognito click starts a child that reloads the current Runtime", () => {
    const loader = readFileSync(join(import.meta.dir, "runtime/incodex-loader.cts"), "utf8");
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    expect(loader).toContain("const current = JSON.parse(fs.readFileSync(currentPath, \"utf8\"));");
    expect(main).toContain("child = spawn(bin, args");
    expect(main).toContain('INCODEX_INCOGNITO: "1"');
    expect(main).toContain("CODEX_ELECTRON_USER_DATA_PATH: session.chromium");
    expect(main).toContain("const args = [`--user-data-dir=$" + "{session.chromium}`]");
    expect(main).toContain("safeHome.handoffSessionOwner");
  });

  test("an ordinary incognito click marks its session pending before child handoff", () => {
    const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    expect(main).toContain("handoffPending: true");
  });
});
