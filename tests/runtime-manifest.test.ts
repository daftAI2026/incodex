import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";

describe("runtime manifest", () => {
  test("committed dist includes version and content hashes", () => {
    const path = join(import.meta.dir, "../dist/runtime-manifest.json");
    expect(existsSync(path)).toBe(true);
    const manifest = JSON.parse(readFileSync(path, "utf8")) as {
      runtimeVersion?: string;
      files?: Record<string, string>;
    };
    expect(manifest.runtimeVersion).toMatch(/^\d+\.\d+\.\d+/);
    expect(manifest.files?.["incodex-loader.cjs"]).toMatch(/^[0-9a-f]{64}$/);
    expect(manifest.files?.["incodex-main.cjs"]).toMatch(/^[0-9a-f]{64}$/);
    expect(manifest.files?.["incodex-preload.cjs"]).toMatch(/^[0-9a-f]{64}$/);
  });

  test("compiled runtime CJS has no machine-specific paths and uses __dirname", () => {
    const loader = readFileSync(join(import.meta.dir, "../dist/incodex-loader.cjs"), "utf8");
    const main = readFileSync(join(import.meta.dir, "../dist/incodex-main.cjs"), "utf8");
    expect(loader).toContain("__dirname");
    expect(loader).not.toMatch(/\/Users\/|\/home\/[^.]|file:\/\/\//);
    expect(main).not.toMatch(/\/Users\/|\/home\/[^.]|file:\/\/\//);
  });
});
