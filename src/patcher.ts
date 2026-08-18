import { readFileSync } from "node:fs";
import { join } from "node:path";
import { patchAsar } from "./asar";
import { writeAsarIntegrity } from "./integrity";
import { ASAR_REL } from "./paths";

export type RuntimeArtifacts = {
  loader: string;
  inject: string;
  main: string;
  preload: string;
  safeHome: string;
  ipcGuard: string;
  instance: string;
  runtimeLoad: string;
  windowKind: string;
};

export function loadRuntimeArtifacts(repoRoot: string): RuntimeArtifacts {
  return {
    loader: readFileSync(join(repoRoot, "dist/incodex-loader.cjs"), "utf8"),
    inject: readFileSync(join(repoRoot, "dist/incodex-inject.js"), "utf8"),
    main: readFileSync(join(repoRoot, "dist/incodex-main.cjs"), "utf8"),
    preload: readFileSync(join(repoRoot, "dist/incodex-preload.cjs"), "utf8"),
    safeHome: readFileSync(join(repoRoot, "dist/incodex-safe-home.cjs"), "utf8"),
    ipcGuard: readFileSync(join(repoRoot, "dist/incodex-ipc-guard.cjs"), "utf8"),
    instance: readFileSync(join(repoRoot, "dist/incodex-instance.cjs"), "utf8"),
    runtimeLoad: readFileSync(join(repoRoot, "dist/incodex-runtime-load.cjs"), "utf8"),
    windowKind: readFileSync(join(repoRoot, "dist/incodex-window-kind.cjs"), "utf8"),
  };
}

export async function patchStagedBundle(options: {
  stagedApp: string;
  artifacts: RuntimeArtifacts;
  installId: string;
}): Promise<{ originalMain: string; hash: string }> {
  const patched = await patchAsar({
    asarPath: join(options.stagedApp, ASAR_REL),
    loaderSource: options.artifacts.loader,
    installId: options.installId,
  });
  writeAsarIntegrity(options.stagedApp, patched.hash);
  return patched;
}
