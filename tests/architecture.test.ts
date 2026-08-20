import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");

function src(rel: string): string {
  return readFileSync(join(root, rel), "utf8");
}

describe("architecture boundaries", () => {
  test("native CLI integration and release cutover boundary stay documented", () => {
    const agents = src("AGENTS.md");
    const contributing = src("CONTRIBUTING.md");
    const safetyReviewer = src(".claude/agents/safety-reviewer.md");
    const releaseFlow = src(".claude/skills/release-flow/SKILL.md");
    const releaseWorkflow = src(".github/workflows/release.yml");
    const readme = src("README.md");
    const readmeCn = src("README_CN.md");
    expect(agents).toContain("Rust workspace now lives on `main`");
    expect(agents).toContain("The next stable Release will publish the native Rust CLI");
    expect(agents).toContain("New releases do not publish legacy Bun CLI assets");
    expect(agents).toContain("Never delete old Release assets");
    expect(agents).toContain("New Rust CLI PRs target `main`");
    expect(agents).toContain("tests/cli-golden.test.ts");
    expect(agents).toContain("Incognito-window hover");
    expect(agents).toContain("failing `cargo test` repro commit");
    expect(agents).toContain("Do not use CDP as the everyday Dock / `install` launch path");
    expect(agents).toContain("`incodex open` may start the official binary with `--remote-debugging-port`");
    expect(agents).not.toContain("Do not add Overlay, CDP-as-launcher");
    expect(agents).not.toContain("--base exp/rust-cli");
    expect(agents).not.toContain("exp/rust-cli");
    expect(agents).toContain("`crates/incodex-cli` is the native CLI");
    expect(agents).toContain("`crates/incodex-core/src/session.rs`");
    expect(contributing).toContain("Rust CLI source is on `main`");
    expect(contributing).toContain("Rust CLI PRs use base `main`");
    expect(contributing).toContain("the next stable release will publish the native Rust CLI");
    expect(contributing).toContain("do not publish new legacy Bun CLI assets");
    expect(contributing).toContain("failing `cargo test` repro");
    expect(contributing).toContain("cargo run -p incodex-cli -- --help");
    expect(safetyReviewer).toContain("crates/incodex-cli/src/install.rs");
    expect(safetyReviewer).toContain("crates/incodex-transaction/**");
    expect(releaseFlow).not.toContain("if this is not 0.1.0");
    expect(readme).not.toContain("Runtime      0.1.0");
    expect(readmeCn).not.toContain("Runtime      0.1.0");
    if (releaseWorkflow.includes("bun build src/cli.ts --compile")) {
      expect(readme).not.toContain("the hat-glasses control and banner still appear in that window");
      expect(readmeCn).not.toContain("这一扇里仍有帽子按钮和提示横幅");
    }
  });

  test("legacy TypeScript CLI cannot remain a second live updater", () => {
    const agents = src("AGENTS.md");
    const legacyCli = src("src/cli.ts");

    expect(agents).toContain("The Rust CLI is the sole product CLI");
    expect(agents).toContain("The legacy TypeScript CLI must not perform live self-updates");
    expect(legacyCli).not.toContain("raw.githubusercontent.com/daftAI2026/incodex/main/install.sh");
    expect(legacyCli).not.toMatch(/spawnSync\(\s*["']bash["']/);
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
