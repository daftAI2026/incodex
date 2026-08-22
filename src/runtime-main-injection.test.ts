import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const main = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");

function hookWindowSource(): string {
  const start = main.indexOf("function hookWindow(");
  const end = main.indexOf("\nasync function attachElectron()", start);
  expect(start).toBeGreaterThanOrEqual(0);
  expect(end).toBeGreaterThan(start);
  return main.slice(start, end);
}

describe("Electron UI injection reporting", () => {
  test("keeps recovery injection timing but only did-finish-load requests one report", () => {
    const hook = hookWindowSource();

    expect(hook).toContain('win.webContents.on("dom-ready", () => run(false))');
    expect(hook).toContain('win.webContents.on("did-finish-load", () => run(true))');
    expect(hook).toContain("run(false)");
    expect(hook.match(/run\(true\)/g)).toHaveLength(1);
    expect(hook.match(/reportInjectionProbe/g)).toHaveLength(1);
  });

  test("does not silently swallow executeJavaScript rejection", () => {
    const hook = hookWindowSource();

    expect(hook).not.toContain(".catch(() => {})");
    expect(hook).toMatch(/\.catch\(\(error\) => reportInjectionError\(error\)\)/);
  });
});
