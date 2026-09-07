import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";

const root = join(import.meta.dir, "..");

function read(path: string): string {
  return readFileSync(join(root, path), "utf8").replaceAll("\r\n", "\n");
}

describe("Windows CI integrity", () => {
  test("keeps hash-sensitive repository inputs on LF checkouts", () => {
    const attributes = read(".gitattributes");

    expect(attributes).toMatch(/^\/Cargo\.lock text eol=lf$/m);
    expect(attributes).toMatch(/^\/dist\/\*\* text eol=lf$/m);
  });

  test("makes every multiline Windows boundary fail on its first native error", () => {
    const workflow = read(".github/workflows/ci.yml");
    const windowsJob = workflow.split("\n  windows-cargo:\n").at(1);
    expect(windowsJob, "windows-cargo job must exist").toBeDefined();

    const blocks = [...(windowsJob ?? "").matchAll(/run: \|\n((?: {10}.*\n?)+)/g)].map(
      ([, body]) => body,
    );
    expect(blocks.length).toBeGreaterThan(0);
    expect((windowsJob?.match(/shell: pwsh/g) ?? []).length).toBe(blocks.length);
    for (const body of blocks) {
      expect(body).toContain("$ErrorActionPreference = 'Stop'");
      expect(body).toContain("$PSNativeCommandUseErrorActionPreference = $true");
    }
  });

  test("surfaces official blocker progress across the Windows injection worker boundary", () => {
    const windowsOpen = read("crates/incodex-cli/src/windows_open.rs");

    expect(windowsOpen).toContain("WindowsInjectionProgress::BlockedByOfficialUi");
    expect(windowsOpen).toContain("OFFICIAL_BLOCKER_WAIT_MESSAGE");
    expect(windowsOpen).toContain("progress_tx.send");
  });
});
