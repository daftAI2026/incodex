import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";

const root = join(import.meta.dir, "..");
const script = join(root, "scripts/update-homebrew-tap-formula.sh");
const releaseYml = readFileSync(join(root, ".github/workflows/release.yml"), "utf8");
const readme = readFileSync(join(root, "README.md"), "utf8");

const SAMPLE = `class Incodex < Formula
  desc "Incognito toggle for the Codex desktop app"
  homepage "https://github.com/daftAI2026/incodex"
  version "0.1.0"
  license "MIT"

  depends_on :macos

  if Hardware::CPU.arm?
    url "https://github.com/daftAI2026/incodex/releases/download/v#{version}/incodex-darwin-arm64"
    sha256 "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  elsif Hardware::CPU.intel?
    url "https://github.com/daftAI2026/incodex/releases/download/v#{version}/incodex-darwin-x64"
    sha256 "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  else
    odie "Incodex currently ships macOS Intel and Apple Silicon binaries only"
  end
end
`;

function runScript(args: string[]): ReturnType<typeof spawnSync> {
  return spawnSync("bash", [script, ...args], { encoding: "utf8" });
}

describe("Homebrew tap bump", () => {
  test("update script rewrites version and sha256s", () => {
    const dir = mkdtempSync(join(tmpdir(), "incodex-tap-"));
    const formula = join(dir, "incodex.rb");
    writeFileSync(formula, SAMPLE);
    chmodSync(formula, 0o644);
    const arm = "1".repeat(64);
    const x64 = "2".repeat(64);
    const ran = runScript(["--formula", formula, "--tag", "v0.2.0", "--arm-sha", arm, "--x64-sha", x64]);
    expect(ran.status).toBe(0);
    const out = readFileSync(formula, "utf8");
    expect(out).toContain('version "0.2.0"');
    expect(out).toContain(`sha256 "${arm}"`);
    expect(out).toContain(`sha256 "${x64}"`);
    expect(out).toContain("v#{version}/incodex-darwin-arm64");
    expect(out).toContain("v#{version}/incodex-darwin-x64");
    expect(out).not.toContain('version "0.1.0"');
    expect(out).not.toContain("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    expect(out).not.toContain("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
  });

  test("update script fails closed when the formula is missing", () => {
    const ran = runScript([
      "--formula",
      join(tmpdir(), "no-such-incodex.rb"),
      "--tag",
      "v0.2.0",
      "--arm-sha",
      "a".repeat(64),
      "--x64-sha",
      "b".repeat(64),
    ]);
    expect(ran.status).not.toBe(0);
  });

  test("update script rejects a tag that is not vX.Y.Z", () => {
    const dir = mkdtempSync(join(tmpdir(), "incodex-tap-"));
    const formula = join(dir, "incodex.rb");
    writeFileSync(formula, SAMPLE);
    const ran = runScript([
      "--formula",
      formula,
      "--tag",
      "V0.2.0",
      "--arm-sha",
      "a".repeat(64),
      "--x64-sha",
      "b".repeat(64),
    ]);
    expect(ran.status).not.toBe(0);
    expect(readFileSync(formula, "utf8")).toBe(SAMPLE);
  });

  test("release workflow bumps daftAI2026/homebrew-tap and does not PR core", () => {
    expect(releaseYml).toContain("update-formula:");
    expect(releaseYml).toContain("needs: release");
    expect(releaseYml).toContain("daftAI2026/homebrew-tap");
    expect(releaseYml).toContain("scripts/update-homebrew-tap-formula.sh");
    expect(releaseYml).toMatch(/incodex \$\{VERSION\}/);
    expect(releaseYml).toContain("Automated release via GitHub Actions");
    expect(releaseYml).toContain("HOMEBREW_TAP_TOKEN");
    expect(releaseYml).not.toContain("Homebrew/homebrew-core");
    expect(releaseYml).not.toContain("bump-homebrew-formula-action");
  });

  test("README documents the tap install", () => {
    expect(readme).toContain("brew tap daftAI2026/tap");
    expect(readme).toContain("brew install incodex");
  });
});
