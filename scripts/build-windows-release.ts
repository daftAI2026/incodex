import { copyFileSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const root = join(import.meta.dir, "..");
const WINDOWS_TARGET_BINARY = "target/release/incodex.exe";
const WINDOWS_RELEASE_BINARY = "release-cli/incodex-windows-x64.exe";

export const WINDOWS_X64_MACHINE = 0x8664;

export function peMachine(bytes: Uint8Array): number {
  if (bytes.length < 64 || bytes[0] !== 0x4d || bytes[1] !== 0x5a) {
    throw new Error("Windows release asset is not a valid PE image");
  }

  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const peOffset = view.getUint32(0x3c, true);
  if (peOffset > bytes.length - 6) {
    throw new Error("Windows release asset has an invalid PE header offset");
  }
  if (
    bytes[peOffset] !== 0x50 ||
    bytes[peOffset + 1] !== 0x45 ||
    bytes[peOffset + 2] !== 0 ||
    bytes[peOffset + 3] !== 0
  ) {
    throw new Error("Windows release asset is missing the PE signature");
  }
  return view.getUint16(peOffset + 4, true);
}

function runCargoBuild(): void {
  const result = spawnSync(
    "cargo",
    ["build", "--locked", "--release", "--package", "incodex-cli", "--bin", "incodex"],
    { cwd: root, stdio: "inherit" },
  );
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`cargo build failed with status ${result.status ?? "unknown"}`);
  }
}

function capture(binary: string, args: string[]): string {
  const result = spawnSync(binary, args, {
    cwd: root,
    encoding: "utf8",
    windowsHide: true,
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(
      `${binary} ${args.join(" ")} failed with status ${result.status ?? "unknown"}\n${result.stderr}`,
    );
  }
  return result.stdout;
}

function packageVersion(): string {
  const packageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as {
    version: string;
  };
  return packageJson.version;
}

export function buildWindowsRelease(): void {
  if (process.platform !== "win32") {
    throw new Error("Windows release asset must be built and smoked on Windows");
  }

  runCargoBuild();

  const target = join(root, WINDOWS_TARGET_BINARY);
  const releaseDirectory = join(root, "release-cli");
  const releaseBinary = join(root, WINDOWS_RELEASE_BINARY);
  rmSync(releaseDirectory, { recursive: true, force: true });
  mkdirSync(releaseDirectory, { recursive: true });
  copyFileSync(target, releaseBinary);

  const machine = peMachine(readFileSync(releaseBinary));
  if (machine !== WINDOWS_X64_MACHINE) {
    throw new Error(
      `Windows release asset must be x86_64, found PE machine 0x${machine.toString(16).padStart(4, "0")}`,
    );
  }

  const version = packageVersion();
  const versionOutput = capture(releaseBinary, ["--version"]);
  const firstVersionLine = versionOutput.split(/\r?\n/, 1)[0];
  if (firstVersionLine !== `Incodex version ${version}`) {
    throw new Error(`Windows release asset reported an unexpected version: ${firstVersionLine}`);
  }

  const helpOutput = capture(releaseBinary, ["--help"]);
  if (!helpOutput.includes("incodex — Incognito toggle for Codex desktop")) {
    throw new Error("Windows release asset did not print the expected help text");
  }

  process.stdout.write(`Verified ${WINDOWS_RELEASE_BINARY}\n`);
}

if (import.meta.main) {
  buildWindowsRelease();
}
