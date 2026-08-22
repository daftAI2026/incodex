import { afterEach, describe, expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { prepareRelease } from "../scripts/prepare-release";

const root = join(import.meta.dir, "..");
const tempRoots: string[] = [];
const workspacePackages = [
  "incodex-asar",
  "incodex-cli",
  "incodex-core",
  "incodex-macos",
  "incodex-runtime-bundle",
  "incodex-transaction",
];

function read(rootDir: string, path: string): string {
  return readFileSync(join(rootDir, path), "utf8");
}

function fixture(): string {
  const dir = mkdtempSync(join(tmpdir(), "incodex-release-version-"));
  tempRoots.push(dir);
  mkdirSync(join(dir, "dist"));

  writeFileSync(join(dir, "package.json"), '{\n  "name": "incodex",\n  "version": "0.2.0"\n}\n');
  writeFileSync(join(dir, "Cargo.toml"), '[workspace.package]\nversion = "0.2.0"\nedition = "2021"\n');
  writeFileSync(
    join(dir, "Cargo.lock"),
    workspacePackages.map((name) => `[[package]]\nname = "${name}"\nversion = "0.2.0"\n`).join("\n"),
  );
  writeFileSync(
    join(dir, "dist/runtime-manifest.json"),
    '{\n  "runtimeVersion": "0.2.0",\n  "files": { "incodex-main.cjs": "hash" }\n}\n',
  );
  const readme = [
    "  Runtime      0.2.0",
    "  Runtime      0.2.0 releases/0.2.0-<manifestSha256>",
    "  Version      0.2.0",
    "  External     0.2.0 releases/0.2.0-<manifestSha256>",
    "Incodex version 0.2.0",
    "",
  ].join("\n");
  writeFileSync(join(dir, "README.md"), readme);
  writeFileSync(join(dir, "README_CN.md"), readme);
  return dir;
}

afterEach(() => {
  for (const dir of tempRoots.splice(0)) rmSync(dir, { recursive: true, force: true });
});

describe("release version preparation", () => {
  test("one command synchronizes every version-bearing release file", () => {
    const dir = fixture();
    prepareRelease(dir, "0.3.0");

    expect(JSON.parse(read(dir, "package.json")).version).toBe("0.3.0");
    expect(read(dir, "Cargo.toml")).toContain('version = "0.3.0"');
    expect(JSON.parse(read(dir, "dist/runtime-manifest.json")).runtimeVersion).toBe("0.3.0");
    for (const name of workspacePackages) {
      expect(read(dir, "Cargo.lock")).toContain(`name = "${name}"\nversion = "0.3.0"`);
    }
    for (const path of ["README.md", "README_CN.md"]) {
      expect(read(dir, path)).not.toContain("0.2.0");
      expect(read(dir, path)).toContain("Incodex version 0.3.0");
      expect(read(dir, path)).toContain("releases/0.3.0-<manifestSha256>");
    }
  });

  test("an invalid stable version is rejected before any file changes", () => {
    const dir = fixture();
    const before = read(dir, "package.json");

    expect(() => prepareRelease(dir, "v0.3.0")).toThrow(/X\.Y\.Z/);
    expect(read(dir, "package.json")).toBe(before);
  });

  test("the repository keeps generated versions aligned with package.json", () => {
    const version = (JSON.parse(read(root, "package.json")) as { version: string }).version;
    expect(read(root, "Cargo.toml")).toContain(`version = "${version}"`);
    expect(JSON.parse(read(root, "dist/runtime-manifest.json")).runtimeVersion).toBe(version);
    for (const name of workspacePackages) {
      expect(read(root, "Cargo.lock")).toContain(`name = "${name}"\nversion = "${version}"`);
    }
    for (const path of ["README.md", "README_CN.md"]) {
      expect(read(root, path)).toContain(`Incodex version ${version}`);
      expect(read(root, path)).toContain(`Runtime      ${version}`);
      expect(read(root, path)).toContain(`releases/${version}-<manifestSha256>`);
    }
  });

  test("the runbook exposes the reusable preparation command", () => {
    const pkg = JSON.parse(read(root, "package.json")) as { scripts: Record<string, string> };
    const flow = read(root, ".claude/skills/release-flow/SKILL.md");
    expect(pkg.scripts["release:prepare"]).toBe("bun scripts/prepare-release.ts");
    expect(flow).toContain("bun run release:prepare -- <version>");
  });
});
