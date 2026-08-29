import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";
import { RUNTIME_ARTIFACT_NAMES } from "../src/runtime-manifest.ts";

describe("runtime manifest", () => {
  test("one catalog owns every current Runtime artifact", () => {
    const catalogPath = join(import.meta.dir, "../runtime-artifacts.json");
    expect(existsSync(catalogPath)).toBe(true);
    const catalog = JSON.parse(readFileSync(catalogPath, "utf8")) as {
      loader?: string;
      external?: string[];
    };
    expect(catalog.loader).toBe("incodex-loader.cjs");
    expect(catalog.external).toEqual(
      RUNTIME_ARTIFACT_NAMES.filter((name) => name !== catalog.loader),
    );
    expect(new Set(RUNTIME_ARTIFACT_NAMES).size).toBe(RUNTIME_ARTIFACT_NAMES.length);
    for (const name of RUNTIME_ARTIFACT_NAMES) {
      expect(name).toMatch(/^incodex-[a-z-]+\.(?:cjs|js)$/);
    }
  });

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

  test("platform boundaries consume the catalog instead of copying it", () => {
    const loader = readFileSync(
      join(import.meta.dir, "../src/runtime/incodex-loader.cts"),
      "utf8",
    );
    const bundle = readFileSync(
      join(import.meta.dir, "../crates/incodex-runtime-bundle/src/lib.rs"),
      "utf8",
    );
    const windows = readFileSync(
      join(import.meta.dir, "../crates/incodex-cli/src/windows_runtime.rs"),
      "utf8",
    );
    const archive = readFileSync(
      join(import.meta.dir, "../crates/incodex-asar/src/archive.rs"),
      "utf8",
    );
    const probe = readFileSync(
      join(import.meta.dir, "../crates/incodex-cli/tests/probe.rs"),
      "utf8",
    );

    expect(loader).toContain("__INCODEX_RUNTIME_FILES__");
    expect(bundle).toContain("incodex_runtime_assets::external_files");
    expect(windows).toContain("incodex_runtime_assets::external_files");
    expect(archive).toContain("incodex_runtime_assets::external_artifact_names");
    expect(probe).toContain("incodex_runtime_assets::external_artifact_names");
  });
});
