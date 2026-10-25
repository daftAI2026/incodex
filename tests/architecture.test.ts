/**
 * [INPUT]: 依赖仓库源码、指导文档和发布配置的静态文本。
 * [OUTPUT]: 验证架构依赖、浏览器桥接隔离及原生 CLI/open 边界。
 * [POS]: 仓库结构回归；抽取模块后仍核对完整 renderer 与浏览器产物。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");

function src(rel: string): string {
  return readFileSync(join(root, rel), "utf8");
}

describe("architecture boundaries", () => {
  test("Runtime hot-update design is visible before contributors add startup behavior", () => {
    const agents = src("AGENTS.md"), contributing = src("CONTRIBUTING.md");
    expect(agents).toContain("## Runtime hot-update contract");
    expect(agents.indexOf("## Runtime hot-update contract")).toBeLessThan(agents.indexOf("## Product Direction"));
    expect(agents).toContain("[RUNTIME.md](RUNTIME.md)");
    expect(contributing).toContain("[RUNTIME.md](RUNTIME.md)");
    const contract = src("RUNTIME.md");
    expect(contract).toContain("发布不等于生效");
    expect(contract).toContain("换功能实现，不重建资源所有权");
    expect(contract).toContain("prepare → acknowledge → commit → retire");
    expect(contract).toContain("禁止用清空 require.cache 冒充热更新");
    expect(contract).toContain("当前覆盖范围");
  });
  test("native CLI integration and release cutover boundary stay documented", () => {
    const agents = src("AGENTS.md");
    const contributing = src("CONTRIBUTING.md");
    const safetyReviewer = src(".claude/agents/safety-reviewer.md");
    const releaseFlow = src(".claude/skills/release-flow/SKILL.md");
    const releaseWorkflow = src(".github/workflows/release.yml");
    const readme = src("README.md");
    const readmeCn = src("README_CN.md");
    expect(agents).toContain("Bun builds the embedded Electron Runtime and its checks");
    expect(agents).toContain("it must not produce or launch a shipped TypeScript CLI binary");
    expect(agents).not.toMatch(/\b[a-f0-9]{40}\b/u);
    expect(agents).not.toMatch(/\bRC\d+\b/u);
    expect(agents).not.toMatch(/\bv\d+\.\d+\.\d+\b/u);
    expect(agents).toContain("New releases do not publish legacy Bun CLI assets");
    expect(agents).toContain("Never delete old Release assets");
    expect(agents).toContain("New Rust CLI PRs target `main`");
    expect(agents).toContain("native Rust contract tests");
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
    expect(contributing).toContain("v0.3.1 published the native Rust CLI");
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

  test("retired TypeScript router cannot remain a product entry", () => {
    const agents = src("AGENTS.md");
    const packageJson = JSON.parse(src("package.json")) as {
      bin?: Record<string, string>;
      scripts?: Record<string, string>;
    };

    expect(agents).toContain("The Rust CLI is the sole product CLI");
    expect(agents).toContain(
      "The TypeScript product router, parser, mutation implementation, and old Runtime publishers have been retired",
    );
    expect(existsSync(join(root, "src/cli.ts"))).toBe(false);
    expect(existsSync(join(root, "src/parse-cli.ts"))).toBe(false);
    expect(packageJson.bin?.incodex).toBeUndefined();
    expect(packageJson.bin?.inc).toBeUndefined();
    expect(packageJson.scripts?.incodex).toBeUndefined();
  });

  test("UI adapter has no file deletion, process control, or auth handling", () => {
    const inject = src("src/runtime/inject.ts");
    const bridge = src("src/runtime/renderer-action-bridge.ts");
    const retry = src("src/runtime/renderer-device-key-activation.ts");
    for (const renderer of [inject, bridge, retry, src("dist/incodex-inject.js")]) {
      expect(renderer).not.toMatch(/node:fs|node:child_process|auth\.json|rmSync|spawn\(/);
    }
    expect(inject).toContain("createRendererActionBridge");
    expect(bridge).toContain("requestIncognitoAction");
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

  test("native installer owns patching while Bun only builds Runtime", () => {
    const install = src("crates/incodex-cli/src/install.rs");
    const runtime = src("src/build-runtime.ts");
    expect(install).toContain("incodex_asar::");
    expect(install).not.toMatch(/Command::new\(["']bun["']\)/);
    expect(runtime).toContain("src/runtime/inject.ts");
    expect(runtime).toContain("writeRuntimeManifest");
    expect(runtime).toContain("process.execPath");
    expect(runtime).not.toContain(
      'spawnSync(join(root, "node_modules/typescript/bin/tsc")',
    );
  });

  test("native open path does not patch asar or resign", () => {
    const open = src("crates/incodex-cli/src/open.rs").replace(/\r\n/g, "\n");
    const command = src("crates/incodex-cli/src/open_command.rs");
    const process = src("crates/incodex-cli/src/open_process.rs").replace(/\r\n/g, "\n");
    const spawn = src("crates/incodex-cli/src/open_process_spawn.rs");
    const cdp = src("crates/incodex-cli/src/cdp.rs").replace(/\r\n/g, "\n");

    for (const module of [open, command, process, spawn]) {
      expect(module).not.toContain("incodex_asar");
      expect(module).not.toContain("codesign");
      expect(module).not.toContain("patch_asar");
    }
    expect(open).toContain('#[path = "open_command.rs"]\nmod command;');
    expect(open).toContain('pub use command::run_open;');
    expect(open).toContain('#[path = "open_process.rs"]\nmod process;');
    expect(cdp).toContain('#[path = "cdp_injection.rs"]\nmod injection;');
    expect(process).toMatch(/let injection = inject_shared_ui_with_retained_session\(\s*port,/);
    expect(process).toContain("session.wait_for_disconnect(&process_alive)");
  });
});
