import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";
import { RUNTIME_ARTIFACT_NAMES } from "../src/runtime-manifest.ts";

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

  test("runtime builds normalize generated CJS line endings", () => {
    const buildRuntime = readFileSync(
      join(import.meta.dir, "../src/build-runtime.ts"),
      "utf8",
    );
    expect(buildRuntime).toContain('.replaceAll("\\r\\n", "\\n")');
    expect(buildRuntime).toContain('.replaceAll("\\r", "\\n")');
  });

  test("every shared runtime artifact is covered by each platform boundary", () => {
    const externalNames = RUNTIME_ARTIFACT_NAMES.filter(
      (name) => name !== "incodex-loader.cjs",
    );
    const sources = [
      "../src/runtime/incodex-loader.cts",
      "../crates/incodex-runtime-bundle/src/lib.rs",
      "../crates/incodex-cli/src/windows_runtime.rs",
      "../crates/incodex-asar/src/archive.rs",
      "../crates/incodex-cli/tests/probe.rs",
    ].map((path) => readFileSync(join(import.meta.dir, path), "utf8"));

    for (const name of externalNames) {
      for (const source of sources) expect(source).toContain(`"${name}"`);
    }
  });
});
