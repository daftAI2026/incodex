import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { publishExternalRuntimeFromDist } from "./external-runtime";
import type { RuntimeArtifacts } from "./patcher";

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
  return join(dirname(fileURLToPath(import.meta.url)), "..", "dist");
}

export function packagedRuntimeVersion(distDir: string): string {
  const path = join(distDir, "runtime-manifest.json");
  if (!existsSync(path)) throw new Error("missing dist runtime file: runtime-manifest.json");
  const raw = JSON.parse(readFileSync(path, "utf8")) as { runtimeVersion?: string };
  if (typeof raw.runtimeVersion !== "string" || !raw.runtimeVersion) {
    throw new Error("runtime-manifest.json missing runtimeVersion");
  }
  return raw.runtimeVersion;
}

function readDistFile(distDir: string, name: string): string {
  const path = join(distDir, name);
  if (!existsSync(path)) throw new Error(`missing dist runtime file: ${name}`);
  return readFileSync(path, "utf8");
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

export function publishPackagedRuntime(userRoot: string, distDir: string) {
  return publishExternalRuntimeFromDist(userRoot, distDir, packagedRuntimeVersion(distDir));
}
