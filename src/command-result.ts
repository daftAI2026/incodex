export type CommandResult = {
  action: "install" | "uninstall" | "runtime";
  skipped?: boolean;
  installId?: string;
  runtimeVersion?: string;
  app?: string;
};

export function formatCommandResult(result: CommandResult): string {
  const lines: string[] = [];
  if (result.skipped) lines.push("already current");
  if (result.installId) lines.push(`install id: ${result.installId}`);
  if (result.runtimeVersion) lines.push(`runtime version: ${result.runtimeVersion}`);
  if (result.app) lines.push(`app: ${result.app}`);
  return lines.join("\n");
}
