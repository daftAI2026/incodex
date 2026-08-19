export type ConfirmDecision = "ok" | "ask" | "require-yes";

export function confirmDecision(options: {
  clone: boolean;
  dryRun: boolean;
  yes: boolean;
  tty: boolean;
}): ConfirmDecision {
  if (options.clone || options.dryRun) return "ok";
  if (options.yes) return "ok";
  if (options.tty) return "ask";
  return "require-yes";
}

export function isTty(
  stdin: { isTTY?: boolean } = process.stdin,
  stdout: { isTTY?: boolean } = process.stdout,
): boolean {
  return Boolean(stdin.isTTY && stdout.isTTY);
}

export function requireYesMessage(command: "install" | "uninstall"): string {
  return `non-interactive ${command} requires --yes
  incodex ${command} --yes`;
}
