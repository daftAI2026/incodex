import { existsSync } from "node:fs";
import { join } from "node:path";
import { asarHasOnlyLoader, readPackageMain } from "./asar";
import { formatKv, formatOk, formatWarn } from "./cli-print";
import { inspectExternalRuntime } from "./external-runtime";
import { loadCurrentInstallation, targetId } from "./installation";
import { ASAR_REL, DEFAULT_APP, USER_ROOT } from "./paths";
import { loadState } from "./state";

export type StatusView = {
  appPath: string;
  exists: boolean;
  patched: boolean;
  loaderOnly: boolean | null;
  runtime: string;
  main: string;
  installId?: string;
  targetId: string;
  appVersion?: string;
};

export function formatStatus(view: StatusView): string {
  const lines = [
    formatKv("App", view.appPath),
    formatKv("Exists", view.exists ? "yes" : "no"),
    formatKv("Installed", view.patched ? "yes" : "no"),
  ];
  if (view.loaderOnly !== null) lines.push(formatKv("Loader", view.loaderOnly ? "asar loader only" : "mixed"));
  lines.push(formatKv("Runtime", view.runtime));
  if (view.appVersion) lines.push(formatKv("Version", view.appVersion));
  if (view.installId) lines.push(formatKv("Install id", view.installId));
  lines.push(formatKv("Target", view.targetId));
  if (view.main) lines.push(formatKv("Main", view.main));
  return lines.join("\n");
}

export function printStatus(appPath = DEFAULT_APP): void {
  const asarPath = join(appPath, ASAR_REL);
  if (!existsSync(appPath)) {
    console.log(formatWarn(`Codex app not found: ${appPath}`));
    return;
  }
  if (!existsSync(asarPath)) {
    console.log(formatStatus({
      appPath,
      exists: true,
      patched: false,
      loaderOnly: null,
      runtime: "missing",
      main: "",
      targetId: targetId(appPath),
    }));
    console.log(formatWarn("asar missing"));
    return;
  }
  const pkg = readPackageMain(asarPath);
  const runtime = inspectExternalRuntime(USER_ROOT);
  const stored = loadCurrentInstallation(appPath);
  const state = loadState();
  const version = stored
    ? `${stored.manifest.appVersion} ${stored.manifest.appBuild}`.trim()
    : state?.appVersion
      ? `${state.appVersion} ${state.appBuild ?? ""}`.trim()
      : undefined;
  console.log(
    formatStatus({
      appPath,
      exists: true,
      patched: pkg.alreadyPatched,
      loaderOnly: asarHasOnlyLoader(asarPath),
      runtime: runtime.ok ? `${runtime.version} ${runtime.release}` : runtime.present ? "invalid" : "missing",
      main: pkg.main,
      installId: pkg.installId ?? stored?.manifest.installId,
      targetId: targetId(appPath),
      appVersion: version,
    }),
  );
  if (pkg.alreadyPatched) console.log(formatOk("Incodex is installed. Use doctor for hashes and signing."));
}
