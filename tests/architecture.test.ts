import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");

function src(rel: string): string {
  return readFileSync(join(root, rel), "utf8");
}

describe("architecture boundaries", () => {
  test("native CLI experiment is documented so crate PRs do not target main", () => {
    const agents = src("AGENTS.md");
    const contributing = src("CONTRIBUTING.md");
    expect(agents).toContain("exp/rust-cli");
    expect(agents).toContain("tests/cli-golden.test.ts");
    expect(agents).toContain("--base exp/rust-cli");
    expect(agents).toContain("Do not send crate PRs at `main`");
    expect(agents).toContain("Incognito-window hover");
    expect(contributing).toContain("exp/rust-cli");
    expect(contributing).toContain("base exp/rust-cli");
    expect(contributing).toContain("Rust crate PRs use base `exp/rust-cli`");
  });

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
      expect(file).not.toContain("26.810.52044");
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
    expect(src("src/asar.ts")).toContain("ASAR_RUNTIME_LEFTOVERS");
    expect(src("src/patcher.ts")).toContain("loaderSource: options.artifacts.loader");
    expect(src("src/patcher.ts")).not.toContain("mainSource:");
  });

  test("installer does not rebuild runtime by spawning bun", () => {
    const install = src("src/install.ts");
    expect(install).not.toContain("src/build-runtime.ts");
    expect(install).not.toMatch(/spawnSync\(\s*["']bun["']/);
  });

  test("open path does not patch asar or resign", () => {
    const open = src("src/open-incognito.ts");
    expect(open).not.toContain('from "./asar"');
    expect(open).not.toContain('from "./codesign"');
    expect(open).not.toContain("signApp");
    expect(open).not.toContain("patchAsar");
  });
});

