import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const releaseYml = readFileSync(join(root, ".github/workflows/release.yml"), "utf8");
const packageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as {
  scripts?: Record<string, string>;
};

describe("release CLI artifacts", () => {
  test("cross-compiles the native Rust CLI into the stable macOS asset names", () => {
    expect(releaseYml).toContain(
      "actions-rust-lang/setup-rust-toolchain@166cdcfd11aee3cb47222f9ddb555ce30ddb9659 # v1.17.0",
    );
    expect(releaseYml).toContain("target: aarch64-apple-darwin,x86_64-apple-darwin");
    expect(releaseYml).toContain("cargo build --locked --release --target aarch64-apple-darwin");
    expect(releaseYml).toContain("cargo build --locked --release --target x86_64-apple-darwin");
    expect(releaseYml).toContain("target/aarch64-apple-darwin/release/incodex");
    expect(releaseYml).toContain("target/x86_64-apple-darwin/release/incodex");
    expect(releaseYml).toContain("incodex-darwin-arm64");
    expect(releaseYml).toContain("incodex-darwin-x64");
    expect(releaseYml).not.toContain("bun build src/cli.ts --compile");
    expect(packageJson.scripts?.["build:cli"]).toBeUndefined();
    expect(existsSync(join(root, "scripts/build-cli.ts"))).toBe(false);
  });

  test("fails closed unless tag, package, workspace, runtime, architecture, and signature agree", () => {
    expect(releaseYml).toContain('[[ "$TAG" =~ ^v([0-9]+\\.[0-9]+\\.[0-9]+)$ ]]');
    expect(releaseYml).toContain('[[ "$PACKAGE_VERSION" == "$VERSION" ]]');
    expect(releaseYml).toContain('[[ "$CARGO_VERSION" == "$VERSION" ]]');
    expect(releaseYml).toContain('[[ "$RUNTIME_VERSION" == "$VERSION" ]]');
    expect(releaseYml).toContain("rm -rf release-cli");
    expect(releaseYml).toContain("file release-cli/incodex-darwin-arm64");
    expect(releaseYml).toContain("file release-cli/incodex-darwin-x64");
    expect(releaseYml).toContain("lipo release-cli/incodex-darwin-arm64 -verify_arch arm64");
    expect(releaseYml).toContain("lipo release-cli/incodex-darwin-x64 -verify_arch x86_64");
    expect(releaseYml).not.toContain("lipo -verify_arch");
    expect(releaseYml).toContain("codesign --verify --strict release-cli/incodex-darwin-arm64");
    expect(releaseYml).toContain("codesign --verify --strict release-cli/incodex-darwin-x64");
    expect(releaseYml).toContain("unexpected release asset set");
  });

  test("binds the built runtime manifest to the release commit", () => {
    expect(releaseYml).toContain('SOURCE_COMMIT="${' + 'GITHUB_SHA}"');
    expect(releaseYml).toContain('[[ "$SOURCE_COMMIT" =~ ^[0-9a-f]{40}$ ]]');
    expect(releaseYml).toContain('[[ "$BUILT_SOURCE_COMMIT" == "$SOURCE_COMMIT" ]]');
  });

  test("smoke-tests the host binary and verifies the published runtime pointer", () => {
    expect(releaseYml).toContain('case "$(uname -m)" in');
    expect(releaseYml).toContain('"$ARM_BINARY" --version');
    expect(releaseYml).toContain('"$ARM_BINARY" --help');
    expect(releaseYml).toContain('"$ARM_BINARY" runtime');
    expect(releaseYml).toContain("current.schemaVersion !== 1");
    expect(releaseYml).toContain("manifestSha256");
    expect(releaseYml).toContain("sourceCommit");
    expect(releaseYml).toContain("runtime-manifest.json");
    expect(releaseYml).toContain("crypto.createHash(\"sha256\")");
    expect(releaseYml).toContain("current.release");
  });

  test("smokes both stable assets on the arm64 release host", () => {
    expect(releaseYml).toContain('case "$(uname -m)" in');
    expect(releaseYml).toContain("arm64) ;;");
    expect(releaseYml).toContain('ARM_BINARY="release-cli/incodex-darwin-arm64"');
    expect(releaseYml).toContain('X64_BINARY="release-cli/incodex-darwin-x64"');
    expect(releaseYml).toContain('"$ARM_BINARY" --version');
    expect(releaseYml).toContain('"$ARM_BINARY" --help >/dev/null');
    expect(releaseYml).toContain('HOME="$ARM_SMOKE_HOME" "$ARM_BINARY" runtime');
    expect(releaseYml).toContain('/usr/bin/arch -x86_64 "$X64_BINARY" --version');
    expect(releaseYml).toContain('/usr/bin/arch -x86_64 "$X64_BINARY" --help >/dev/null');
    expect(releaseYml).toContain(
      'HOME="$X64_SMOKE_HOME" /usr/bin/arch -x86_64 "$X64_BINARY" runtime',
    );
    expect(releaseYml).toContain('verify_runtime_pointer "$ARM_SMOKE_HOME"');
    expect(releaseYml).toContain('verify_runtime_pointer "$X64_SMOKE_HOME"');
  });

  test("smoke validates the exact ten-file Runtime set", () => {
    expect(releaseYml).toContain("const REQUIRED_RUNTIME_FILES = [");
    for (const name of [
      "incodex-main.cjs",
      "incodex-preload.cjs",
      "incodex-inject.js",
      "incodex-safe-home.cjs",
      "incodex-ipc-guard.cjs",
      "incodex-owner-core.cjs",
      "incodex-owner-recovery.cjs",
      "incodex-instance.cjs",
      "incodex-window-kind.cjs",
      "incodex-runtime-load.cjs",
    ]) {
      expect(releaseYml).toContain(`"${name}"`);
    }
    expect(releaseYml).toContain('currentFileNames.join("\\0")');
    expect(releaseYml).toContain('manifestFileNames.join("\\0")');
    expect(releaseYml).toContain('REQUIRED_RUNTIME_FILES.slice().sort().join("\\0")');
    expect(releaseYml).toContain("manifest.files[name] !== expected");
  });

  test("publishes only the two stable Rust assets and their checksums", () => {
    expect(releaseYml).toContain("SHA256SUMS");
    expect(releaseYml).toMatch(/files:[\s\S]*incodex-darwin-arm64/);
    expect(releaseYml).toMatch(/files:[\s\S]*incodex-darwin-x64/);
    expect(releaseYml).toMatch(/files:[\s\S]*SHA256SUMS/);
    expect(releaseYml).not.toContain("checksums.txt");
    expect(releaseYml).not.toMatch(/files:[\s\S]*runtime-manifest\.json/);
    expect(releaseYml).not.toMatch(/legacy[-_ ].*bun|bun[-_ ].*legacy/i);
  });

  test("creates the GitHub Release without auto-generated notes", () => {
    expect(releaseYml).toContain("generate_release_notes: false");
    expect(releaseYml).not.toContain("generate_release_notes: true");
  });
});
