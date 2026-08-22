import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const releaseYml = readFileSync(join(root, ".github/workflows/release.yml"), "utf8");
const packageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as {
  scripts?: Record<string, string>;
};
const runtimeManifest = JSON.parse(readFileSync(join(root, "dist/runtime-manifest.json"), "utf8")) as {
  files: Record<string, string>;
};
const manifestFileNames = Object.keys(runtimeManifest.files).sort();
const externalFileNames = manifestFileNames.filter((name) => name !== "incodex-loader.cjs");

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

  test("behavior-smokes each final signed Rust asset with the same ignored harness", () => {
    const harnessPath = join(root, "crates/incodex-cli/tests/release_asset_smoke.rs");
    expect(existsSync(harnessPath)).toBe(true);
    const harness = existsSync(harnessPath) ? readFileSync(harnessPath, "utf8") : "";

    const workflow = releaseYml.replace(/\\\n\s*/g, " ");
    const command =
      "cargo test --locked --release --package incodex-cli --test release_asset_smoke -- --ignored --exact release_asset_behavior_smoke";
    const armInvocation =
      `INCODEX_RELEASE_BINARY="$PWD/$ARM_BINARY" INCODEX_RELEASE_ARCH="arm64" ${command}`;
    const x64Invocation =
      `INCODEX_RELEASE_BINARY="$PWD/$X64_BINARY" INCODEX_RELEASE_ARCH="x86_64" ${command}`;
    const finalSignature = workflow.indexOf(
      "codesign --sign - --force release-cli/incodex-darwin-x64",
    );

    expect(finalSignature).toBeGreaterThanOrEqual(0);
    expect(workflow.indexOf(armInvocation)).toBeGreaterThan(finalSignature);
    expect(workflow.indexOf(x64Invocation)).toBeGreaterThan(finalSignature);

    expect(harness).toContain("INCODEX_RELEASE_BINARY");
    expect(harness).toContain("INCODEX_RELEASE_ARCH");
    expect(harness).toContain("Command::new");
    expect(harness).toContain('arg("--version")');
    expect(harness).toContain('arg("--help")');
    expect(harness).toContain('args(["status", "--json"])');
    expect(harness).toContain('args(["open", "--dry-run"])');
    expect(harness).toContain('args(["install", "--yes"])');
    expect(harness).toContain('args(["uninstall", "--yes"])');
    expect(harness).toMatch(/original_snapshot/);
    expect(harness).toMatch(/restored_snapshot/);
    expect(harness).toMatch(/assert_eq!\(\s*restored_snapshot,\s*original_snapshot/);
  });

  test("smoke validates external and manifest Runtime file sets separately", () => {
    expect(externalFileNames).toHaveLength(10);
    expect(manifestFileNames).toHaveLength(11);
    expect(manifestFileNames.filter((name) => !externalFileNames.includes(name))).toEqual([
      "incodex-loader.cjs",
    ]);
    expect(releaseYml).toContain("const REQUIRED_EXTERNAL_FILES = [");
    expect(releaseYml).toContain("const REQUIRED_MANIFEST_FILES = [");
    for (const name of manifestFileNames) {
      expect(releaseYml).toContain(`"${name}"`);
    }
    expect(releaseYml).toContain(
      'currentFileNames.join("\\0") !== REQUIRED_EXTERNAL_FILES.slice().sort().join("\\0")',
    );
    expect(releaseYml).toContain(
      'manifestFileNames.join("\\0") !== REQUIRED_MANIFEST_FILES.slice().sort().join("\\0")',
    );
    expect(releaseYml).toContain("manifest.files[name] !== expected");
    expect(releaseYml).toContain(
      'const loaderManifestHash = manifest.files["incodex-loader.cjs"];',
    );
    expect(releaseYml).toContain('if (!/^[0-9a-f]{64}$/.test(loaderManifestHash))');
    expect(releaseYml).not.toContain('path.join(release, "incodex-loader.cjs")');
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
