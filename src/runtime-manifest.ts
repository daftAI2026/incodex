import { writeFileSync } from "node:fs";
import { join } from "node:path";

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
