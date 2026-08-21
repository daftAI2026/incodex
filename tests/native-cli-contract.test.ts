import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";

const root = join(import.meta.dir, "..");
const frozenTypeScriptFixtures = new Set([
  "legacy_typescript.rs",
  "support/legacy_typescript_matrix.rs",
]);

function text(relativePath: string): string {
  return readFileSync(join(root, relativePath), "utf8");
}

function rustTestSources(relativeDir: string): string[] {
  const absoluteDir = join(root, relativeDir);
  const sources: string[] = [];
  const visit = (directory: string, prefix: string) => {
    for (const entry of readdirSync(directory).sort()) {
      const absolutePath = join(directory, entry);
      const relativePath = join(prefix, entry);
      if (statSync(absolutePath).isDirectory()) {
        visit(absolutePath, relativePath);
      } else if (entry.endsWith(".rs")) {
        sources.push(relativePath);
      }
    }
  };
  visit(absoluteDir, "");
  return sources;
}

function isLegacyFixtureSource(relativePath: string): boolean {
  return frozenTypeScriptFixtures.has(relativePath);
}

describe("native CLI contract boundary", () => {
  test("does not execute the retired TypeScript product CLI or observe global ChatGPT", () => {
    expect(existsSync(join(root, "tests/cli-golden.test.ts"))).toBe(false);

    const sources = rustTestSources("crates/incodex-cli/tests").filter(
      (relativePath) => !isLegacyFixtureSource(relativePath),
    );
    expect(sources).toContain("probe.rs");
    expect(sources).toContain("readonly.rs");
    expect(sources).toContain("support/tty.rs");
    expect(sources).toContain("legacy_proof.rs");
    expect(sources).toContain("legacy_uninstall.rs");
    expect(sources).not.toContain("legacy_typescript.rs");
    expect(sources).not.toContain("support/legacy_typescript_matrix.rs");
    expect(isLegacyFixtureSource("legacy_typescript.rs")).toBe(true);
    expect(isLegacyFixtureSource("support/legacy_typescript_matrix.rs")).toBe(true);
    expect(isLegacyFixtureSource("legacy_anything.rs")).toBe(false);
    expect(isLegacyFixtureSource("legacy_proof.rs")).toBe(false);
    expect(isLegacyFixtureSource("legacy_uninstall.rs")).toBe(false);
    for (const relativePath of sources) {
      const source = text(join("crates/incodex-cli/tests", relativePath));
      expect(source).not.toMatch(/\bbun\b/);
      expect(source).not.toContain("run_ts");
      expect(source).not.toContain("listOfficialPids");
      expect(source).not.toMatch(/Command::new\(["']ps["']\)/);
      expect(source).not.toContain("/Applications/ChatGPT.app/Contents/MacOS/ChatGPT");
      expect(source).not.toContain("ChatGPT is running");
    }
  });

  test("documents native contract tests and the minimal legacy fixture boundary", () => {
    const readme = text("tests/README.md");
    const agents = text("AGENTS.md");
    expect(readme).toContain("native CLI contract");
    expect(readme).toContain("legacy_typescript.rs");
    expect(agents).toContain("The Rust CLI is the sole product CLI");
    expect(agents).toContain("minimum frozen fixture");
    expect(agents).not.toContain("tests/cli-golden.test.ts");
  });
});
