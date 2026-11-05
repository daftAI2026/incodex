/**
 * [INPUT]: 依赖 CI 路径选路函数与工作流声明。
 * [OUTPUT]: 验证手册独立门和未知或混合改动保留产品门。
 * [POS]: CI 选路回归；不执行发布或修改 GitHub 设置。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { shardFiles } from "../scripts/ci-test-shard.ts";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { requiresProductChecks, changedPaths } from "../scripts/ci-change-scope.ts";

// Only the public manual has its own complete build/audit workflow.
describe("CI manual-only routing", () => {
  test("manual source, lockfiles, assets and deletions use the manual pipeline", () => {
    expect(requiresProductChecks(["manual/src/content/docs/zh/index.mdx", "manual/package-lock.json", "manual/public/images/deleted.svg"])).toBe(false);
  });
  test("mixed changes and every unclassified surface retain product checks", () => {
    for (const path of ["src/runtime/inject.ts", "crates/incodex-cli/src/open.rs", ".github/workflows/ci.yml", "scripts/ci-change-scope.ts", "manual-other/file", "AGENTS.md"]) {
      expect(requiresProductChecks(["manual/releases.json", path])).toBe(true);
    }
  });
  test("prose-only changes skip product checks but instructions remain code-sensitive", () => {
    expect(requiresProductChecks(["README.md", "README_CN.md", "CONTRIBUTING.md", "SECURITY.md"])).toBe(false);
    expect(requiresProductChecks(["AGENTS.md"])).toBe(true);
    expect(requiresProductChecks(["README.md", "src/runtime/inject.ts"])).toBe(true);
  });
  test("missing evidence fails closed", () => {
    expect(requiresProductChecks([])).toBe(true);
    expect(changedPaths("invalid", "invalid")).toBeNull();
    expect(requiresProductChecks(null)).toBe(true);
  });
  test("the required check fails closed across every product job", () => {
    const workflow = readFileSync(new URL("../.github/workflows/ci.yml", import.meta.url), "utf8").replaceAll("\r\n", "\n");
    const gate = workflow.split("\n  check:\n")[1]?.split(/\n {2}[a-z][a-z-]*:\n/)[0];
    expect(gate).toContain("if: always()");
    expect(gate).toContain("needs: [changes, quality, macos-runtime, cargo, windows-cargo]");
    expect(gate).toContain('result === "success"');
    expect(workflow).toContain("format('run-{0}', github.run_id)");
    expect(workflow).toContain("cancel-in-progress: ${{ github.event_name == 'pull_request' }}");
  });
});

describe("CI partition and aggregate behavior", () => {
  test("shards preserve every current macOS test exactly once", () => {
    const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
    const command = pkg.scripts["test:macos"];
    const expected = command.split(/\s+/).slice(2).sort();
    const actual = [0, 1].flatMap(shard => shardFiles(command, shard, 2)).sort();
    expect(actual).toEqual(expected);
    expect(new Set(actual).size).toBe(actual.length);
    for (const args of [[-1, 2], [2, 2], [0, 0], [0.5, 2], [0, 999]]) {
      expect(() => shardFiles(command, args[0]!, args[1]!)).toThrow();
    }
    expect(() => shardFiles("bun test", 0, 2)).toThrow();
  });
  test("aggregate rejects failures, cancellations and unexpected skips", () => {
    const workflow = readFileSync(new URL("../.github/workflows/ci.yml", import.meta.url), "utf8").replaceAll("\r\n", "\n");
    const script = workflow.split("node <<'JS'\n")[1]!.split("\n          JS")[0]!;
    const makeNeeds = () => ({
      changes: { result: "success", outputs: { product: "true" } },
      quality: { result: "success" }, "macos-runtime": { result: "success" },
      cargo: { result: "success" }, "windows-cargo": { result: "success" },
    });
    const run = (needs: object) => spawnSync(process.execPath, ["-e", script], {
      env: { ...process.env, NEEDS_JSON: JSON.stringify(needs) },
    }).status;
    expect(run(makeNeeds())).toBe(0);
    for (const result of ["failure", "cancelled", "skipped"]) {
      const needs = makeNeeds(); needs.cargo.result = result;
      expect(run(needs)).toBe(1);
    }
    const docs = makeNeeds(); docs.changes.outputs.product = "false";
    for (const [name, job] of Object.entries(docs)) if (name !== "changes") job.result = "skipped";
    expect(run(docs)).toBe(0);
    docs.changes.result = "failure";
    expect(run(docs)).toBe(1);
  });
});
