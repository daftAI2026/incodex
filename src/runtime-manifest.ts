import { writeFileSync } from "node:fs";
import { join } from "node:path";

export const RUNTIME_ARTIFACT_NAMES = [
  "incodex-inject.js",
  "incodex-loader.cjs",
  "incodex-main.cjs",
  "incodex-preload.cjs",
  "incodex-safe-home.cjs",
  "incodex-ipc-guard.cjs",
  "incodex-owner-core.cjs",
  "incodex-owner-recovery.cjs",
  "incodex-instance.cjs",
  "incodex-runtime-load.cjs",
  "incodex-window-kind.cjs",
] as const;

export type RuntimeManifest = {
  runtimeVersion: string;
  sourceCommit: string;
  files: Record<string, string>;
};

export const RUNTIME_MANIFEST_NAME = "runtime-manifest.json";

export function writeRuntimeManifest(outDir: string, manifest: RuntimeManifest): string {
  const path = join(outDir, RUNTIME_MANIFEST_NAME);
  writeFileSync(path, `${JSON.stringify(manifest, null, 2)}\n`);
  return path;
}
