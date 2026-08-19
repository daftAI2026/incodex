import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import {
  EXTERNAL_RUNTIME_FILES,
  publishExternalRuntime,
  publishExternalRuntimeFromDist,
  verifyExternalRuntime,
} from "./external-runtime";
import type { RuntimeArtifacts } from "./patcher";

// bun `with { type: "file" }` is a path string; tsc types these files as modules.
// @ts-expect-error
import bundledLoader from "../dist/incodex-loader.cjs" with { type: "file" };
// @ts-expect-error
import bundledInject from "../dist/incodex-inject.js" with { type: "file" };
// @ts-expect-error
import bundledMain from "../dist/incodex-main.cjs" with { type: "file" };
// @ts-expect-error
import bundledPreload from "../dist/incodex-preload.cjs" with { type: "file" };
// @ts-expect-error
import bundledSafeHome from "../dist/incodex-safe-home.cjs" with { type: "file" };
// @ts-expect-error
import bundledIpcGuard from "../dist/incodex-ipc-guard.cjs" with { type: "file" };
// @ts-expect-error
import bundledInstance from "../dist/incodex-instance.cjs" with { type: "file" };
// @ts-expect-error
import bundledRuntimeLoad from "../dist/incodex-runtime-load.cjs" with { type: "file" };
// @ts-expect-error
import bundledWindowKind from "../dist/incodex-window-kind.cjs" with { type: "file" };
import bundledManifest from "../dist/runtime-manifest.json";

const BUNDLED_FILES: Record<string, string> = {
  "incodex-loader.cjs": bundledLoader,
  "incodex-inject.js": bundledInject,
  "incodex-main.cjs": bundledMain,
  "incodex-preload.cjs": bundledPreload,
  "incodex-safe-home.cjs": bundledSafeHome,
  "incodex-ipc-guard.cjs": bundledIpcGuard,
  "incodex-instance.cjs": bundledInstance,
  "incodex-runtime-load.cjs": bundledRuntimeLoad,
  "incodex-window-kind.cjs": bundledWindowKind,
};

const ARTIFACT_FILES = {
  loader: "incodex-loader.cjs",
  inject: "incodex-inject.js",
  main: "incodex-main.cjs",
  preload: "incodex-preload.cjs",
  safeHome: "incodex-safe-home.cjs",
  ipcGuard: "incodex-ipc-guard.cjs",
  instance: "incodex-instance.cjs",
  runtimeLoad: "incodex-runtime-load.cjs",
  windowKind: "incodex-window-kind.cjs",
} as const;

export function defaultPackagedDistDir(): string {
  return dirname(bundledLoader);
}

export function resolvePackagedDistDir(env: NodeJS.Dict<string> = process.env): string {
  if (env.INCODEX_DIST) return env.INCODEX_DIST;
  return defaultPackagedDistDir();
}

function usesBundledFiles(distDir: string, env: NodeJS.Dict<string> = process.env): boolean {
  return !env.INCODEX_DIST && distDir === defaultPackagedDistDir();
}

function readDistFile(distDir: string, name: string, env: NodeJS.Dict<string> = process.env): string {
  const path = usesBundledFiles(distDir, env) ? BUNDLED_FILES[name] : join(distDir, name);
  if (!path || !existsSync(path)) throw new Error(`missing dist runtime file: ${name}`);
  return readFileSync(path, "utf8");
}

export function packagedRuntimeVersion(distDir: string): string {
  if (usesBundledFiles(distDir)) {
    if (typeof bundledManifest.runtimeVersion !== "string" || !bundledManifest.runtimeVersion) {
      throw new Error("runtime-manifest.json missing runtimeVersion");
    }
    return bundledManifest.runtimeVersion;
  }
  const raw = JSON.parse(readDistFile(distDir, "runtime-manifest.json")) as { runtimeVersion?: string };
  if (typeof raw.runtimeVersion !== "string" || !raw.runtimeVersion) {
    throw new Error("runtime-manifest.json missing runtimeVersion");
  }
  return raw.runtimeVersion;
}

export function loadPackagedArtifacts(distDir: string): RuntimeArtifacts {
  return {
    loader: readDistFile(distDir, ARTIFACT_FILES.loader),
    inject: readDistFile(distDir, ARTIFACT_FILES.inject),
    main: readDistFile(distDir, ARTIFACT_FILES.main),
    preload: readDistFile(distDir, ARTIFACT_FILES.preload),
    safeHome: readDistFile(distDir, ARTIFACT_FILES.safeHome),
    ipcGuard: readDistFile(distDir, ARTIFACT_FILES.ipcGuard),
    instance: readDistFile(distDir, ARTIFACT_FILES.instance),
    runtimeLoad: readDistFile(distDir, ARTIFACT_FILES.runtimeLoad),
    windowKind: readDistFile(distDir, ARTIFACT_FILES.windowKind),
  };
}

function distFileHash(distDir: string, name: string): string {
  return createHash("sha256").update(readDistFile(distDir, name)).digest("hex");
}

export function runtimeMatchesPackaged(userRoot: string, distDir = resolvePackagedDistDir()): boolean {
  try {
    const existing = verifyExternalRuntime(userRoot);
    if (existing.current.version !== packagedRuntimeVersion(distDir)) return false;
    return EXTERNAL_RUNTIME_FILES.every((name) => existing.current.files[name] === distFileHash(distDir, name));
  } catch {
    return false;
  }
}

export function publishPackagedRuntime(userRoot: string, distDir: string) {
  const version = packagedRuntimeVersion(distDir);
  try {
    const existing = verifyExternalRuntime(userRoot);
    if (existing.current.version === version) {
      const same = EXTERNAL_RUNTIME_FILES.every((name) => existing.current.files[name] === distFileHash(distDir, name));
      if (same) return existing.current;
    }
  } catch {
    /* missing or invalid runtime; publish */
  }
  if (usesBundledFiles(distDir)) {
    const files: Record<string, string> = {};
    for (const name of EXTERNAL_RUNTIME_FILES) {
      files[name] = readDistFile(distDir, name);
    }
    return publishExternalRuntime({
      userRoot,
      version,
      files,
    });
  }
  return publishExternalRuntimeFromDist(userRoot, distDir, version);
}
