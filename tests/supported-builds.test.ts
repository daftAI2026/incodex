import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";

const root = join(import.meta.dir, "..");

function src(rel: string): string {
  return readFileSync(join(root, rel), "utf8");
}

describe("Codex builds are not an allowlist", () => {
  test("there is no version+build table", () => {
    expect(existsSync(join(root, "src/compatibility/supported-builds.ts"))).toBe(false);
  });

  test("native install does not warn about an unknown Codex build number", () => {
    expect(src("crates/incodex-cli/src/install.rs")).not.toContain("unknown Codex build");
    expect(src("crates/incodex-cli/src/install.rs")).not.toContain("findSupportedBuild");
  });

  test("the renderer policy has no adapter registry or observed-build implementation", () => {
    expect(existsSync(join(root, "src/runtime/compatibility/default-adapter.ts"))).toBe(false);
    expect(existsSync(join(root, "src/runtime/compatibility/registry.ts"))).toBe(false);
    expect(existsSync(join(root, "src/runtime/compatibility/types.ts"))).toBe(false);
    expect(existsSync(join(root, "src/runtime/compatibility/build-26.810.52044.ts"))).toBe(false);
    expect(src("src/runtime/compatibility/search-labels.ts")).not.toMatch(/26\.810\.52044|6662/);
  });
});
