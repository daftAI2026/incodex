/**
 * [INPUT]: 依赖 package.json 的显式套件入口与测试文件清单。
 * [OUTPUT]: 验证套件归属、平台选路、本地实签验收与冻结fixture换行。
 * [POS]: 测试入口和字节固定夹具的合同守卫；新增测试不得绕过完整检查。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join, relative } from "node:path";
import { runInNewContext } from "node:vm";

const repoRoot = join(import.meta.dir, "..");
const packageJsonPath = join(repoRoot, "package.json");
const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf8")) as {
  scripts?: Record<string, string>;
};

const suiteNames = ["shared", "macos", "windows"] as const;
const suiteScriptNames = suiteNames.map((suite) => `test:${suite}`);
const platformScriptName = "test:platform";
const macOsPermissionTests = [
  "src/runtime-install-permission.test.ts",
  "src/runtime/permission-probe.test.ts",
];

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
      /(?:src|tests|scripts)\/[A-Za-z0-9_./-]+\.test\.[cm]?[jt]sx?/g,
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
    const testFiles = [...testFilesUnder("src"), ...testFilesUnder("tests"), ...testFilesUnder("scripts")].sort();
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

  test("routes macOS install preflight and POSIX permission tests only to macOS", () => {
    const scripts = packageJson.scripts ?? {};

    for (const file of macOsPermissionTests) {
      const owners = suiteNames.filter((suite) =>
        referencedTestFiles(scripts[`test:${suite}`] ?? "").includes(file),
      );
      expect(owners, `${file} must run in the macOS suite`).toEqual(["macos"]);
    }
  });

  test("preserves the byte-exact frozen Runtime fixture as LF in Git checkouts", () => {
    const fixture = "crates/incodex-cli/tests/fixtures/runtime-load-rc11.cjs";
    const attributes = execFileSync("git", ["check-attr", "text", "eol", "--", fixture], {
      cwd: repoRoot,
      encoding: "utf8",
    });

    expect(attributes.trim().split(/\r?\n/)).toEqual([
      `${fixture}: text: set`,
      `${fixture}: eol: lf`,
    ]);
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


// --- 环境实签归显式本地验收；默认门仍执行同文件其他合同。 ---
describe("local certificate-signing acceptance boundary", () => {
  test("requires explicit macOS opt-in for the real certificate Runtime case", () => {
    const source = readFileSync(join(repoRoot, "src/runtime/remote-key-compat.test.ts"), "utf8");
    const condition = source.match(/test\.skipIf\(([^\n]+)\)\("host identity verifies real local signatures/)?.[1];
    expect(condition).toBeDefined();
    for (const [platform, value, skipped] of [
      ["darwin", undefined, true],
      ["darwin", "0", true],
      ["darwin", "true", true],
      ["darwin", "1", false],
      ["win32", "1", true],
    ] as const) {
      const env = value === undefined ? {} : { INCODEX_LOCAL_SIGNING_ACCEPTANCE: value };
      expect(runInNewContext(condition!, { process: { platform, env } })).toBe(skipped);
    }
  });

  test("retains both real Rust signing scenarios as explicitly runnable ignored acceptance", () => {
    const source = readFileSync(join(repoRoot, "crates/incodex-cli/tests/macos_signing_context.rs"), "utf8");
    const ignored = [...source.matchAll(/#\[ignore = "([^"\n]+)"\]\r?\nfn ([a-z_]+)\(/g)];
    expect(ignored.map((match) => match[2])).toEqual([
      "production_context_signs_each_sparkle_domain_component_and_keeps_its_dr_across_payloads",
      "context_verifier_rejects_a_different_certificate_and_identifier_only_dr",
    ]);
    for (const match of ignored) expect(match[1]).toContain("local certificate-signing acceptance");
  });
  test("keeps certificate-signed registration scenarios in the same explicit acceptance lane", () => {
    const source = readFileSync(join(repoRoot, "crates/incodex-cli/tests/macos_signing_registration.rs"), "utf8");
    const ignored = [...source.matchAll(/#\[ignore = "([^"\n]+)"\]\r?\nfn ([a-z_]+)\(/g)];
    expect(ignored.map((match) => match[2])).toEqual([
      "only_a_new_install_epoch_can_promote_legacy_none_after_matching_local_proof",
      "a_registered_local_epoch_cannot_downgrade_on_wrong_signature_or_damaged_material",
    ]);
    for (const match of ignored) expect(match[1]).toContain("local certificate-signing acceptance");
  });

});
