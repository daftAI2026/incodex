import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");

function src(rel: string): string {
  return readFileSync(join(root, rel), "utf8");
}

describe("architecture boundaries", () => {
  test("UI adapter has no file deletion, process control, or auth handling", () => {
    const inject = src("src/runtime/inject.ts");
    expect(inject).not.toMatch(/node:fs|node:child_process|auth\.json|rmSync|spawn\(/);
    expect(inject).toContain("requestIncognitoAction");
    expect(inject).toContain("showLaunchError");
  });

  test("session manager does not import ASAR or DOM adapters", () => {
    const main = src("src/runtime/incodex-main.cts");
    const instance = src("src/runtime/incodex-instance.cts");
    const home = src("src/runtime/incodex-safe-home.cts");
    for (const file of [main, instance, home]) {
      expect(file).not.toMatch(/from ["']\.\.\/asar["']|require\(["']\.\/asar["']\)/);
      expect(file).not.toContain("querySelector");
      expect(file).not.toContain("compatibility/build-");
    }
  });

  test("patcher is a staged transform and does not touch the official app or home overrides", () => {
    const patcher = src("src/patcher.ts");
    expect(patcher).toContain("export async function patchStagedBundle");
    expect(patcher).not.toContain("DEFAULT_APP");
    expect(patcher).not.toContain("signApp");
    expect(patcher).not.toContain("swapBundle");
    expect(patcher).not.toContain("USER_ROOT");
    expect(patcher).not.toContain(".incodex/targets");
    expect(src("src/install.ts")).toContain("patchStagedBundle");
  });
});
