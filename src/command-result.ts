import { formatKv, formatOk } from "./cli-print";

export type CommandResult = {
  action: "install" | "uninstall" | "runtime";
  skipped?: boolean;
  installId?: string;
  runtimeVersion?: string;
  app?: string;
};

export function formatCommandResult(result: CommandResult): string {
  const lines: string[] = [];
  if (result.skipped) lines.push(formatOk("Already current. Codex was not re-signed."));
  if (result.installId) lines.push(formatKv("Install id", result.installId));
  if (result.runtimeVersion) lines.push(formatKv("Runtime", result.runtimeVersion));
  if (result.app) lines.push(formatKv("App", result.app));
  return lines.join("\n");
}
