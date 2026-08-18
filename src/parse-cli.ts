export type CliCommand = "help" | "install" | "uninstall" | "status" | "doctor" | "recover" | "runtime";

export type ParsedCli = {
  command: CliCommand;
  clone: boolean;
  live: boolean;
  confirmLive: boolean;
  json: boolean;
  app?: string;
  transaction?: string;
};

export function parseCli(argv: string[]): ParsedCli {
  const command = parseCommand(argv[2]);
  const flags = argv.slice(3);
  const clone = flags.includes("--clone");
  const live = flags.includes("--live");
  const confirmLive = flags.includes("--confirm-live");
  const json = flags.includes("--json");
  const app = valueAfter(flags, "--app");
  const transaction = valueAfter(flags, "--transaction");

  if (clone && live) {
    throw new Error("--clone and --live cannot be used together");
  }
  if (command === "uninstall" && !clone && !live && !app) {
    throw new Error("uninstall requires an explicit --live, --clone, or --app <path>");
  }
  if (command === "install" && !clone && !live && !app) {
    throw new Error("install requires --clone, --live, or --app <path>");
  }
  if (command === "install" && live && !confirmLive) {
    throw new Error("install --live requires --confirm-live after you review the planned action");
  }
  if (command === "recover" && !transaction) {
    throw new Error("recover requires --transaction <id>");
  }
  return { command, clone, live, confirmLive, json, app, transaction };
}

function parseCommand(raw: string | undefined): CliCommand {
  const command = raw ?? "help";
  if (
    command === "help" ||
    command === "-h" ||
    command === "--help" ||
    command === "install" ||
    command === "uninstall" ||
    command === "status" ||
    command === "doctor" ||
    command === "recover" ||
    command === "runtime"
  ) {
    return command === "-h" || command === "--help" ? "help" : command;
  }
  throw new Error(`unknown command: ${command}`);
}

function valueAfter(flags: string[], name: string): string | undefined {
  const index = flags.indexOf(name);
  if (index === -1) return undefined;
  const value = flags[index + 1];
  if (!value || value.startsWith("-")) {
    throw new Error(`${name} requires a path, not another flag`);
  }
  return value;
}
