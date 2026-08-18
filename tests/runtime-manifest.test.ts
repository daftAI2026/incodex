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
});
