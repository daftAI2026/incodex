import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const stableVersion = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const workspacePackages = [
  "incodex-asar",
  "incodex-cli",
  "incodex-core",
  "incodex-macos",
  "incodex-runtime-bundle",
  "incodex-transaction",
];

function read(root: string, path: string): string {
  return readFileSync(join(root, path), "utf8");
}

function replaceRequired(source: string, pattern: RegExp, replacement: string, label: string): string {
  if (!pattern.test(source)) throw new Error(`cannot find ${label}`);
  pattern.lastIndex = 0;
  return source.replace(pattern, replacement);
}

function updateReadme(source: string, version: string, path: string): string {
  let next = source;
  next = replaceRequired(
    next,
    /^(\s+Runtime\s+)\d+\.\d+\.\d+( releases\/)\d+\.\d+\.\d+(\s*)$/m,
    `$1${version}$2${version}$3`,
    `${path} status Runtime version`,
  );
  next = replaceRequired(
    next,
    /^(\s+Runtime\s+)\d+\.\d+\.\d+(\s*)$/m,
    `$1${version}$2`,
    `${path} install Runtime version`,
  );
  next = replaceRequired(
    next,
    /^(\s+Version\s+)\d+\.\d+\.\d+(\s*)$/m,
    `$1${version}$2`,
    `${path} doctor Runtime version`,
  );
  next = replaceRequired(
    next,
    /^(\s+External\s+)\d+\.\d+\.\d+( releases\/)\d+\.\d+\.\d+(\s*)$/m,
    `$1${version}$2${version}$3`,
    `${path} external Runtime version`,
  );
  return replaceRequired(
    next,
    /^(Incodex version )\d+\.\d+\.\d+(\s*)$/m,
    `$1${version}$2`,
    `${path} CLI version`,
  );
}

export function prepareRelease(root: string, version: string): void {
  if (!stableVersion.test(version)) {
    throw new Error(`release version must be X.Y.Z without a v prefix: ${version}`);
  }

  const packageJson = JSON.parse(read(root, "package.json")) as Record<string, unknown>;
  packageJson.version = version;

  let cargo = read(root, "Cargo.toml");
  cargo = replaceRequired(
    cargo,
    /(\[workspace\.package\][\s\S]*?\nversion = ")[^"]+("[\s\S]*)/,
    `$1${version}$2`,
    "Cargo workspace version",
  );

  let lock = read(root, "Cargo.lock");
  for (const name of workspacePackages) {
    lock = replaceRequired(
      lock,
      new RegExp(`(\\[\\[package\\]\\]\\nname = "${name}"\\nversion = ")[^"]+(")`),
      `$1${version}$2`,
      `Cargo.lock package ${name}`,
    );
  }

  const runtimeManifest = JSON.parse(read(root, "dist/runtime-manifest.json")) as Record<string, unknown>;
  runtimeManifest.runtimeVersion = version;

  const changes = new Map<string, string>([
    ["package.json", `${JSON.stringify(packageJson, null, 2)}\n`],
    ["Cargo.toml", cargo],
    ["Cargo.lock", lock],
    ["dist/runtime-manifest.json", `${JSON.stringify(runtimeManifest, null, 2)}\n`],
    ["README.md", updateReadme(read(root, "README.md"), version, "README.md")],
    ["README_CN.md", updateReadme(read(root, "README_CN.md"), version, "README_CN.md")],
  ]);

  for (const [path, content] of changes) writeFileSync(join(root, path), content);
}

if (import.meta.main) {
  const [version, ...extra] = process.argv.slice(2);
  if (!version || extra.length > 0) {
    process.stderr.write("usage: bun run release:prepare -- X.Y.Z\n");
    process.exit(1);
  }
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  prepareRelease(root, version);
  process.stdout.write(`Prepared Incodex v${version}. Review and commit the changes before tagging.\n`);
}
