import { existsSync, readFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createRequire } from "node:module";
import { runInNewContext } from "node:vm";
import { describe, expect, test } from "bun:test";
import { RUNTIME_ARTIFACT_NAMES } from "../src/runtime-manifest.ts";
import { runtimeCheckEnvironment } from "../scripts/check-dist.ts";

describe("runtime manifest", () => {
  test("dist checks preserve committed provenance without an external override", () => {
    const sourceCommit = "a".repeat(40);
    expect(runtimeCheckEnvironment(sourceCommit, { CI: "true" })).toEqual({
      CI: "true", SOURCE_COMMIT: sourceCommit,
    });
    expect(runtimeCheckEnvironment(sourceCommit, { SOURCE_COMMIT: "" }).SOURCE_COMMIT).toBe(sourceCommit);
  });

  test("dist checks retain an explicit source override and other environment fields", () => {
    const override = "b".repeat(40);
    expect(runtimeCheckEnvironment("a".repeat(40), { SOURCE_COMMIT: override, CI: "true" })).toEqual({
      SOURCE_COMMIT: override, CI: "true",
    });
  });

  test("compiled main resolves the injector and preload beside its own module", () => {
    const directory = join(import.meta.dir, "../dist");
    const mainPath = join(directory, "incodex-main.cjs");
    const module = { exports: {} };
    const nativeRequire = createRequire(mainPath);
    const requireFromMain = (name: string) => {
      if (name === "./incodex-runtime-load.cjs") {
        const runtimeLoad = nativeRequire(name);
        return {
          ...runtimeLoad,
          readVerifiedRuntimeArtifact: (artifact: string, bundledDir: string) =>
            ({ path: join(bundledDir, artifact), bytes: readFileSync(join(bundledDir, artifact)) }),
          readRuntimeJson: (artifact: string, bundledDir: string) =>
            JSON.parse(readFileSync(join(bundledDir, artifact), "utf8")),
          loadRuntimeModule: (artifact: string, bundledDir: string) =>
            nativeRequire(join(bundledDir, artifact)),
        };
      }
      return nativeRequire(name);
    };
    const result = runInNewContext(
      `${readFileSync(mainPath, "utf8")}\n({inject: injectSource(), preload: pickFile("incodex-preload.cjs")})`,
      {
        require: requireFromMain,
        __dirname: directory,
        module,
        exports: module.exports,
        process: { ...process, platform: "test", env: {} },
        console,
      },
    );
    expect(result.inject).toBe(readFileSync(join(directory, "incodex-inject.js"), "utf8"));
    expect(result.preload).toBe(join(directory, "incodex-preload.cjs"));
  });

  test("compiled main pins its release before attach even if publication advances during startup", () => {
    const home = mkdtempSync(join(tmpdir(), "incodex-main-startup-pin-"));
    const runtimeRoot = join(home, ".incodex/runtime");
    const hash = (body: string) => createHash("sha256").update(body).digest("hex");
    function publish(id: string) {
      const bodies = Object.fromEntries(RUNTIME_ARTIFACT_NAMES.filter(name => name !== "incodex-loader.cjs").map(name => [name, readFileSync(join(import.meta.dir, "../dist", name), "utf8")]));
      bodies["incodex-inject.js"] = id;
      const files = Object.fromEntries(Object.entries(bodies).map(([name, body]) => [name, hash(body)]));
      const manifest = JSON.stringify({ runtimeVersion: "1.3.4", sourceCommit: "", files });
      const manifestSha256 = hash(manifest), release = `releases/1.3.4-${manifestSha256}`;
      const directory = join(runtimeRoot, release);
      mkdirSync(directory, { recursive: true });
      for (const [name, body] of Object.entries(bodies)) writeFileSync(join(directory, name), body);
      writeFileSync(join(directory, "runtime-manifest.json"), manifest);
      writeFileSync(join(runtimeRoot, "current.json"), JSON.stringify({ schemaVersion: 1, version: "1.3.4", sourceCommit: "", files, release, manifestSha256 }));
      return directory;
    }
    try {
      const directory = publish("A");
      const mainPath = join(import.meta.dir, "../dist/incodex-main.cjs"), nativeRequire = createRequire(mainPath);
      const runtimeLoad = nativeRequire("./incodex-runtime-load.cjs"), calls: string[] = [];
      const requireFromMain = (name: string) => {
        if (name === "electron") throw new Error("no Electron in this verifier regression");
        if (name === "./incodex-runtime-load.cjs") return { ...runtimeLoad,
          loadRuntimeModule: (artifact: string, bundledDir: string) =>
            runtimeLoad.loadRuntimeModule(artifact, bundledDir, { HOME: home }),
          readRuntimeJson(artifact: string, bundledDir: string) {
            calls.push(artifact);
            const value = runtimeLoad.readRuntimeJson(artifact, bundledDir, { HOME: home });
            publish("B"); return value;
          },
          readVerifiedRuntimeArtifact(artifact: string, bundledDir: string) {
            calls.push(artifact);
            return runtimeLoad.readVerifiedRuntimeArtifact(artifact, bundledDir, { HOME: home });
          },
        };
        return nativeRequire(name);
      };
      const result = runInNewContext(`${readFileSync(mainPath, "utf8")}\ninjectSource()`, {
        require: requireFromMain, __dirname: directory, module: { exports: {} }, exports: {},
        process: { ...process, platform: "test", env: {} }, console,
      });
      expect(calls).toEqual(["incodex-permission-copy.json", "incodex-inject.js"]);
      expect(result).toBe("A");
    } finally { rmSync(home, { recursive: true, force: true }); }
  });

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
      expect(name).toMatch(/^incodex-[a-z-]+\.(?:cjs|js|json)$/);
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
