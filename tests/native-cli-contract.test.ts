import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";

const root = join(import.meta.dir, "..");

function text(relativePath: string): string {
  return readFileSync(join(root, relativePath), "utf8");
}

describe("native CLI contract boundary", () => {
  test("does not execute the retired TypeScript product CLI or observe global ChatGPT", () => {
    expect(existsSync(join(root, "tests/cli-golden.test.ts"))).toBe(false);

    const nativeContract = text("crates/incodex-cli/tests/native_contract.rs");
    const nativeTty = text("crates/incodex-cli/tests/support/native_tty.rs");
    for (const source of [nativeContract, nativeTty]) {
      expect(source).not.toContain("bun src/cli.ts");
      expect(source).not.toContain("run_ts");
      expect(source).not.toContain("/Applications/ChatGPT.app/Contents/MacOS/ChatGPT");
      expect(source).not.toContain("ChatGPT is running");
    }
  });

  test("documents native contract tests and the minimal legacy fixture boundary", () => {
    const readme = text("tests/README.md");
    const agents = text("AGENTS.md");
    expect(readme).toContain("native CLI contract");
    expect(readme).toContain("legacy_typescript.rs");
    expect(agents).toContain("The Rust CLI is the sole product CLI");
    expect(agents).toContain("minimum frozen fixture");
    expect(agents).not.toContain("tests/cli-golden.test.ts");
  });
});
