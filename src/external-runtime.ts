import { createHash } from "node:crypto";
import {
  chmodSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { RUNTIME_CURRENT_NAME, RUNTIME_DIR_NAME, RUNTIME_RELEASES_NAME } from "./paths";

export const EXTERNAL_RUNTIME_FILES = [
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
] as const;

export type ExternalRuntimeFile = (typeof EXTERNAL_RUNTIME_FILES)[number];

export type ExternalCurrent = {
  schemaVersion: 1;
  version: string;
  release: string;
  files: Record<string, string>;
};

const DIR_MODE = 0o700;
const FILE_MODE = 0o600;

export function runtimeRoot(userRoot: string): string {
  return join(userRoot, RUNTIME_DIR_NAME);
}

export function fileSha256(path: string): string {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

export function parseExternalCurrent(raw: unknown): ExternalCurrent {
  if (!raw || typeof raw !== "object") throw new Error("invalid runtime current.json");
  const rec = raw as Partial<ExternalCurrent>;
  if (rec.schemaVersion !== 1) throw new Error("unsupported runtime current.json schema");
  if (typeof rec.version !== "string" || !rec.version) throw new Error("runtime current.json missing version");
  if (typeof rec.release !== "string" || !rec.release) throw new Error("runtime current.json missing release");
  if (rec.release.includes("..") || rec.release.startsWith("/") || rec.release.includes("\\")) {
    throw new Error("runtime release path must be a relative child of the runtime directory");
  }
  if (!rec.files || typeof rec.files !== "object") throw new Error("runtime current.json missing files");
  const files: Record<string, string> = {};
  for (const name of EXTERNAL_RUNTIME_FILES) {
    const hash = rec.files[name];
    if (typeof hash !== "string" || !/^[0-9a-f]{64}$/.test(hash)) {
      throw new Error(`runtime current.json missing sha256 for ${name}`);
    }
    files[name] = hash;
  }
  return { schemaVersion: 1, version: rec.version, release: rec.release, files };
}

function assertNotSymlink(path: string, label: string): void {
  let stats: ReturnType<typeof lstatSync>;
  try {
    stats = lstatSync(path);
  } catch (error) {
    const err = error as NodeJS.ErrnoException;
    if (err.code === "ENOENT") throw new Error(`missing ${label}: ${path}`);
    throw error;
  }
  if (stats.isSymbolicLink()) throw new Error(`refuse to use symlink ${label}: ${path}`);
}

function assertInsideRuntime(path: string, root: string): void {
  const rel = relative(root, path);
  if (rel.startsWith("..") || rel === "") {
    throw new Error(`runtime path escaped runtime root: ${path}`);
  }
}

export function verifyExternalRuntime(userRoot: string): { current: ExternalCurrent; main: string } {
  const root = runtimeRoot(userRoot);
  assertNotSymlink(root, "runtime root");
  const currentPath = join(root, RUNTIME_CURRENT_NAME);
  assertNotSymlink(currentPath, "current.json");
  const current = parseExternalCurrent(JSON.parse(readFileSync(currentPath, "utf8")));
  const releaseDir = resolve(root, current.release);
  assertInsideRuntime(releaseDir, root);
  assertNotSymlink(releaseDir, "release directory");
  for (const name of EXTERNAL_RUNTIME_FILES) {
    const file = join(releaseDir, name);
    assertNotSymlink(file, name);
    const actual = fileSha256(file);
    if (actual !== current.files[name]) {
      throw new Error(`runtime hash mismatch: ${name}`);
    }
  }
  return { current, main: join(releaseDir, "incodex-main.cjs") };
}

export function resolveExternalMain(
  env: { HOME?: string; INCODEX_DEV_HOT?: string },
  execPath = "",
): string {
  const home = env.HOME;
  if (typeof home !== "string" || home.length === 0) {
    throw new Error("HOME is unset; refusing to load Incodex runtime");
  }
  if (env.INCODEX_DEV_HOT === "1") {
    const id = createHash("sha256")
      .update(execPath || "unknown")
      .digest("hex")
      .slice(0, 12);
    const override = join(home, ".incodex", "targets", id, "incodex-main.cjs");
    if (existsSync(override) && !lstatSync(override).isSymbolicLink()) return override;
  }
  return verifyExternalRuntime(join(home, ".incodex")).main;
}

export function publishExternalRuntime(options: {
  userRoot: string;
  files: Record<string, string>;
  version: string;
}): ExternalCurrent {
  const root = runtimeRoot(options.userRoot);
  if (existsSync(root) && lstatSync(root).isSymbolicLink()) {
    throw new Error(`refuse to use symlink runtime root: ${root}`);
  }
  mkdirSync(root, { recursive: true, mode: DIR_MODE });
  chmodSync(root, DIR_MODE);

  const releaseRel = `${RUNTIME_RELEASES_NAME}/${options.version}`;
  const releasesDir = join(root, RUNTIME_RELEASES_NAME);
  mkdirSync(releasesDir, { recursive: true, mode: DIR_MODE });
  if (lstatSync(releasesDir).isSymbolicLink()) {
    throw new Error(`refuse to use symlink releases directory: ${releasesDir}`);
  }

  const staging = mkdtempSync(join(releasesDir, `.staging-${options.version}-`));
  chmodSync(staging, DIR_MODE);
  const hashes: Record<string, string> = {};
  try {
    for (const name of EXTERNAL_RUNTIME_FILES) {
      const body = options.files[name];
      if (typeof body !== "string") throw new Error(`missing runtime artifact: ${name}`);
      const dest = join(staging, name);
      writeFileSync(dest, body, { mode: FILE_MODE });
      chmodSync(dest, FILE_MODE);
      hashes[name] = createHash("sha256").update(body).digest("hex");
    }
    const dest = join(root, releaseRel);
    if (existsSync(dest)) {
      if (lstatSync(dest).isSymbolicLink()) throw new Error(`refuse to replace symlink release: ${dest}`);
      rmSync(dest, { recursive: true, force: true });
    }
    renameSync(staging, dest);
  } catch (error) {
    rmSync(staging, { recursive: true, force: true });
    throw error;
  }

  const current: ExternalCurrent = {
    schemaVersion: 1,
    version: options.version,
    release: releaseRel.split(sep).join("/"),
    files: hashes,
  };
  const currentPath = join(root, RUNTIME_CURRENT_NAME);
  if (existsSync(currentPath) && lstatSync(currentPath).isSymbolicLink()) {
    throw new Error(`refuse to overwrite symlink current.json: ${currentPath}`);
  }
  const tmp = join(dirname(currentPath), `.${RUNTIME_CURRENT_NAME}.tmp`);
  writeFileSync(tmp, `${JSON.stringify(current, null, 2)}\n`, { mode: FILE_MODE });
  chmodSync(tmp, FILE_MODE);
  renameSync(tmp, currentPath);
  return current;
}

export type ExternalRuntimeReport = {
  present: boolean;
  ok: boolean;
  version: string | null;
  release: string | null;
  error: string | null;
};

export function inspectExternalRuntime(userRoot: string): ExternalRuntimeReport {
  const currentPath = join(runtimeRoot(userRoot), RUNTIME_CURRENT_NAME);
  if (!existsSync(currentPath)) {
    return { present: false, ok: false, version: null, release: null, error: "missing current.json" };
  }
  try {
    const { current } = verifyExternalRuntime(userRoot);
    return { present: true, ok: true, version: current.version, release: current.release, error: null };
  } catch (error) {
    return {
      present: true,
      ok: false,
      version: null,
      release: null,
      error: error instanceof Error ? error.message : String(error),
    };
  }
}

export function publishExternalRuntimeFromDist(userRoot: string, distDir: string, version: string): ExternalCurrent {
  return publishExternalRuntime({
    userRoot,
    version,
    files: loadDistRuntimeFiles(distDir),
  });
}

export function loadDistRuntimeFiles(distDir: string): Record<string, string> {
  const files: Record<string, string> = {};
  for (const name of EXTERNAL_RUNTIME_FILES) {
    const path = join(distDir, name);
    if (!existsSync(path)) throw new Error(`missing dist runtime file: ${name}`);
    files[name] = readFileSync(path, "utf8");
  }
  return files;
}
