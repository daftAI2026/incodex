import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";

const repoRoot = join(import.meta.dir, "..");
const packageJsonPath = join(repoRoot, "package.json");
const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf8")) as {
  scripts?: Record<string, string>;
};

const suiteNames = ["shared", "macos", "windows"] as const;
const suiteScriptNames = suiteNames.map((suite) => `test:${suite}`);
const platformScriptName = "test:platform";

function testFilesUnder(directory: string): string[] {
  const absoluteDirectory = join(repoRoot, directory);
  const entries = readdirSync(absoluteDirectory, { withFileTypes: true });
  const files: string[] = [];

  for (const entry of entries) {
    const absolutePath = join(absoluteDirectory, entry.name);
    if (entry.isDirectory()) {
      files.push(...testFilesUnder(join(directory, entry.name)));
      continue;
    }
    if (!entry.isFile() || !/\.test\.[cm]?[jt]sx?$/.test(entry.name)) continue;
    files.push(relative(repoRoot, absolutePath).replaceAll("\\", "/"));
  }

  return files.sort();
}

function commandTokens(command: string): string[] {
  return command
    .replaceAll(/["'`]/g, "")
    .split(/\s+/)
    .map((token) => token.replace(/^\.\//, ""))
    .filter(Boolean);
}

function referencedTestFiles(command: string): string[] {
  return [
    ...dispatcherSources(command.replaceAll("\\", "/")).matchAll(
      /(?:src|tests)\/[A-Za-z0-9_./-]+\.test\.[cm]?[jt]sx?/g,
    ),
  ].map(([file]) => file);
}

function dispatcherSources(command: string): string {
  const sources = [command];
  for (const token of commandTokens(command)) {
    if (!/^scripts\/[^/]+\.[cm]?[jt]sx?$/.test(token)) continue;
    const path = join(repoRoot, token);
    if (existsSync(path)) sources.push(readFileSync(path, "utf8"));
  }
  return sources.join("\n");
}

describe("Bun test suite contract", () => {
  test("declares shared, macOS, and Windows entrypoints", () => {
    const scripts = packageJson.scripts ?? {};

    for (const name of suiteScriptNames) {
      expect(typeof scripts[name], `${name} must be an explicit Bun suite entrypoint`).toBe("string");
      expect(scripts[name], `${name} must invoke Bun test directly`).toMatch(/\bbun\s+test\b/);
    }
  });

  test("assigns every Bun test file to exactly one explicit suite", () => {
    const scripts = packageJson.scripts ?? {};
    const testFiles = [...testFilesUnder("src"), ...testFilesUnder("tests")].sort();
    const ownership = new Map<string, string[]>();

    for (const suite of suiteNames) {
      const command = scripts[`test:${suite}`] ?? "";
      const referenced = referencedTestFiles(command);

      for (const file of referenced) {
        expect(testFiles, `${suite} references a missing test file: ${file}`).toContain(file);
        const owners = ownership.get(file) ?? [];
        owners.push(suite);
        ownership.set(file, owners);
      }
    }

    for (const file of testFiles) {
      expect(ownership.get(file), `${file} must belong to one suite`).toEqual([expect.any(String)]);
    }
    for (const [file, owners] of ownership) {
      expect(owners, `${file} must not belong to multiple suites`).toHaveLength(1);
    }
  });

  test("routes the default check through a platform dispatcher", () => {
    const scripts = packageJson.scripts ?? {};
    const dispatcher = scripts[platformScriptName];
    const check = scripts.check ?? "";

    expect(typeof dispatcher, `${platformScriptName} must exist`).toBe("string");
    expect(check, "check must delegate test selection to the platform dispatcher").toContain(
      `bun run ${platformScriptName}`,
    );

    const source = dispatcherSources(dispatcher ?? "");
    expect(source).toContain("test:shared");
    expect(source).toContain("test:macos");
    expect(source).toContain("test:windows");
    expect(source).toMatch(/process\.platform|win32|darwin|RUNNER_OS|uname\b/);
  });
});
