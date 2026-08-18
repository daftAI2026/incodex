import { describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
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
});
