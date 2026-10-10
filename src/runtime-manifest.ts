/**
 * [INPUT]: 依赖唯一资产 catalog 与文件读写。
 * [OUTPUT]: 导出 loader/外置资产名单与 manifest 类型、写入和读取能力。
 * [POS]: Runtime 构建和验证共用的资产边界；不发布本机 Runtime。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

type RuntimeArtifactCatalog = {
  loader: string;
  external: string[];
};

const catalog = JSON.parse(
  readFileSync(new URL("../runtime-artifacts.json", import.meta.url), "utf8"),
) as RuntimeArtifactCatalog;

export const RUNTIME_LOADER_NAME = catalog.loader;
export const RUNTIME_EXTERNAL_ARTIFACT_NAMES = Object.freeze([...catalog.external]);
export const RUNTIME_ARTIFACT_NAMES = Object.freeze([
  RUNTIME_LOADER_NAME,
  ...RUNTIME_EXTERNAL_ARTIFACT_NAMES,
]);

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
