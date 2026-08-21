import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";

const root = join(import.meta.dir, "..");

type PackageManifest = {
  bin?: Record<string, string>;
  scripts?: Record<string, string>;
};

function read(relativePath: string): string {
  return readFileSync(join(root, relativePath), "utf8");
}

function manifest(): PackageManifest {
  return JSON.parse(read("package.json")) as PackageManifest;
}

const retiredRouterFiles = [
  "src/cli.ts",
  "src/parse-cli.ts",
  "src/help.ts",
  "src/menu.ts",
  "src/menu-update.ts",
  "src/cli-channel.ts",
  "src/confirm.ts",
  "src/confirm-prompt.ts",
  "src/cli-version.ts",
  "src/version-report.ts",
  "src/spinner.ts",
  "src/relaunch.ts",
  "src/status.ts",
  "src/doctor.ts",
  "src/open-incognito.ts",
];

const activeGuides = [
  "AGENTS.md",
  "CONTRIBUTING.md",
  ".claude/skills/release-notes/SKILL.md",
  ".claude/skills/bugs/SKILL.md",
  ".claude/agents/safety-reviewer.md",
];

const retiredGuidePaths = [
  "src/cli.ts",
  "src/parse-cli.ts",
  "src/help.ts",
  "src/menu.ts",
  "src/confirm.ts",
  "src/open-incognito.ts",
  "src/status.ts",
  "src/doctor.ts",
];

describe("native CLI package boundary", () => {
  test("does not publish the retired TypeScript CLI", () => {
    const pkg = manifest();
    expect(pkg.bin?.incodex).toBeUndefined();
    expect(pkg.bin?.inc).toBeUndefined();
    expect(pkg.scripts?.incodex).toBeUndefined();
    for (const [name, command] of Object.entries(pkg.scripts ?? {})) {
      expect(`${name}: ${command}`).not.toContain("bun src/cli.ts");
    }
  });

  test("keeps the Runtime, build, and release toolchain", () => {
    const scripts = manifest().scripts ?? {};
    expect(scripts["release:prepare"]).toBe("bun scripts/prepare-release.ts");
    expect(scripts["build:runtime"]).toBe("bun src/build-runtime.ts");
    expect(scripts["deploy:runtime"]).toBe("bun src/deploy-runtime.ts");
    expect(scripts.typecheck).toBeDefined();
    expect(scripts.lint).toBeDefined();
    expect(scripts.test).toBeDefined();
    expect(scripts.check).toBeDefined();
    expect(scripts["check:dist"]).toBeDefined();
  });

  test("has no TypeScript product router sources", () => {
    for (const relativePath of retiredRouterFiles) {
      expect(() => read(relativePath)).toThrow();
    }
  });

  test("native lifecycle source-checkout guidance does not use Bun links", () => {
    const lifecycle = read("crates/incodex-cli/src/lifecycle.rs");
    expect(lifecycle).not.toContain("bun link");
    expect(lifecycle).not.toContain("bun unlink");
  });

  test("active guides point at native Rust paths", () => {
    for (const guide of activeGuides) {
      const content = read(guide);
      for (const retiredPath of retiredGuidePaths) {
        expect(content).not.toContain(retiredPath);
      }
    }
    expect(read(".claude/skills/release-notes/SKILL.md")).toContain(
      "crates/incodex-cli/src/parse.rs",
    );
    expect(read(".claude/skills/bugs/SKILL.md")).toContain("crates/incodex-cli/src/open.rs");
    expect(read(".claude/agents/safety-reviewer.md")).toContain(
      "crates/incodex-cli/src/open.rs",
    );
  });
});
