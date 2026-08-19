import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const releaseYml = readFileSync(join(import.meta.dir, "..", ".github/workflows/release.yml"), "utf8");

describe("release CLI artifacts", () => {
  test("compiles standalone macOS binaries and publishes checksums", () => {
    expect(releaseYml).toContain("bun build");
    expect(releaseYml).toContain("--compile");
    expect(releaseYml).toContain("incodex-darwin-arm64");
    expect(releaseYml).toContain("incodex-darwin-x64");
    expect(releaseYml).toContain("SHA256SUMS");
    expect(releaseYml).toMatch(/files:[\s\S]*incodex-darwin-arm64/);
    expect(releaseYml).toMatch(/files:[\s\S]*incodex-darwin-x64/);
    expect(releaseYml).toMatch(/files:[\s\S]*SHA256SUMS/);
  });
});
