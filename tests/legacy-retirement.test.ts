import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";

const root = join(import.meta.dir, "..");

function read(relativePath: string): string {
  return readFileSync(join(root, relativePath), "utf8");
}

const retiredSources = [
  "src/app-identity.ts",
  "src/asar.ts",
  "src/canonical-target.ts",
  "src/cli-print.ts",
  "src/codesign.ts",
  "src/command-result.ts",
  "src/external-runtime.ts",
  "src/install-transaction.ts",
  "src/install.ts",
  "src/installation.ts",
  "src/integrity.ts",
  "src/launch-services.ts",
  "src/live-source.ts",
  "src/mutation-lock.ts",
  "src/packaged-runtime.ts",
  "src/patcher.ts",
  "src/paths.ts",
  "src/quit-official.ts",
  "src/recover.ts",
  "src/state.ts",
  "src/swap.ts",
  "src/transaction.ts",
  "src/uninstall.ts",
  "src/app-identity.test.ts",
  "src/asar-fixtures.test.ts",
  "src/asar-unpack.test.ts",
  "src/asar-upgrade.test.ts",
  "src/canonical-target.test.ts",
  "src/codesign.test.ts",
  "src/external-runtime.test.ts",
  "src/install-fault.test.ts",
  "src/install-skip.test.ts",
  "src/installation.test.ts",
  "src/launch-services.test.ts",
  "src/live-source.test.ts",
  "src/mutation-lock.test.ts",
  "src/packaged-runtime.test.ts",
  "src/quit-official.test.ts",
  "src/recover.test.ts",
  "src/swap.test.ts",
  "src/transaction.test.ts",
];

const retiredReferenceTokens = retiredSources.filter((path) => !path.endsWith(".test.ts"));
const activeGraphFiles = [
  "AGENTS.md",
  "CONTRIBUTING.md",
  "README.md",
  "README_CN.md",
  ".claude/agents/safety-reviewer.md",
  ".claude/skills/bugs/SKILL.md",
  ".claude/skills/release-flow/SKILL.md",
  ".github/CODEOWNERS",
  "tests/README.md",
  "tests/architecture.test.ts",
  "tests/native-cli-contract.test.ts",
  "tests/supported-builds.test.ts",
  "scripts/check-dist.ts",
  "scripts/prepare-release.ts",
  "package.json",
];

describe("legacy TypeScript retirement boundary", () => {
  test("mutation and old Runtime publisher sources are absent", () => {
    for (const relativePath of retiredSources) {
      expect(existsSync(join(root, relativePath))).toBe(false);
    }
  });

  test("no active graph can re-introduce the retired sources", () => {
    for (const relativePath of activeGraphFiles) {
      const content = read(relativePath);
      for (const retiredPath of retiredReferenceTokens) {
        expect(content).not.toContain(retiredPath);
      }
    }
  });

  test("the surviving Runtime and native ASAR oracle remain explicit", () => {
    for (const relativePath of [
      "src/build-runtime.ts",
      "src/deploy-runtime.ts",
      "src/runtime-manifest.ts",
      "src/forensics.ts",
      "src/forensics.test.ts",
      "src/runtime/inject.ts",
      "src/runtime/incodex-main.cts",
    ]) {
      expect(existsSync(join(root, relativePath))).toBe(true);
    }

    const packageJson = JSON.parse(read("package.json")) as {
      dependencies?: Record<string, string>;
      scripts?: Record<string, string>;
    };
    expect(packageJson.dependencies?.["@electron/asar"]).toBe("4.2.1");
    expect(read("crates/incodex-asar/tests/fixtures.rs")).toContain('from "@electron/asar"');
    expect(packageJson.scripts?.["build:runtime"]).toBe("bun src/build-runtime.ts");
    expect(packageJson.scripts?.["check:dist"]).toBeDefined();
  });
});
