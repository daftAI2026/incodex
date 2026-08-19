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

  test("install does not warn about an unknown Codex build number", () => {
    expect(src("src/cli.ts")).not.toContain("unknown Codex build");
    expect(src("src/cli.ts")).not.toContain("liveSupportNote");
    expect(src("src/install.ts")).not.toContain("unknown Codex build");
    expect(src("src/install.ts")).not.toContain("findSupportedBuild");
  });

  test("the default adapter is not named after one observed build", () => {
    expect(src("src/runtime/compatibility/default-adapter.ts")).not.toMatch(/26\.810\.52044|6662/);
    expect(src("src/runtime/compatibility/registry.ts")).not.toMatch(/26\.810\.52044|6662/);
    expect(existsSync(join(root, "src/runtime/compatibility/build-26.810.52044.ts"))).toBe(false);
  });
});
