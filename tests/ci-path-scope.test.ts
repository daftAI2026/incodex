import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { requiresProductChecks, changedPaths } from "../scripts/ci-change-scope.ts";

// Only the public manual has its own complete build/audit workflow.
describe("CI manual-only routing", () => {
  test("manual source, lockfiles, assets and deletions use the manual pipeline", () => {
    expect(requiresProductChecks(["manual/src/content/docs/zh/index.mdx", "manual/package-lock.json", "manual/public/images/deleted.svg"])).toBe(false);
  });
  test("mixed changes and every unclassified surface retain product checks", () => {
    for (const path of ["src/runtime/inject.ts", "crates/incodex-cli/src/open.rs", ".github/workflows/ci.yml", "scripts/ci-change-scope.ts", "README.md", "manual-other/file", "AGENTS.md"]) {
      expect(requiresProductChecks(["manual/releases.json", path])).toBe(true);
    }
  });
  test("missing evidence fails closed", () => {
    expect(requiresProductChecks([])).toBe(true);
    expect(changedPaths("invalid", "invalid")).toBeNull();
    expect(requiresProductChecks(null)).toBe(true);
  });
  test("all existing required job names remain and depend on the routing result", () => {
    const workflow = readFileSync(new URL("../.github/workflows/ci.yml", import.meta.url), "utf8").replaceAll("\r\n", "\n");
    for (const name of ["check", "cargo", "windows-cargo"]) {
      const job = workflow.split(`\n  ${name}:\n`)[1]?.split(/\n {2}[a-z][a-z-]*:\n/)[0];
      expect(job).toContain("needs: changes");
      expect(job).toContain("needs.changes.outputs.product != 'false'");
    }
    expect(workflow).toContain("fetch-depth: 0");
  });
});
