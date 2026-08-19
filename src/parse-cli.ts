export type CliCommand =
  | "menu"
  | "help"
  | "version"
  | "install"
  | "uninstall"
  | "status"
  | "doctor"
  | "recover"
  | "runtime"
  | "open"
  | "update"
  | "self-uninstall";

export type ParsedCli = {
  command: CliCommand;
  help: boolean;
  clone: boolean;
  live: boolean;
  yes: boolean;
  dryRun: boolean;
  json: boolean;
  restoreApp: boolean;
  app?: string;
  transaction?: string;
};

const COMMANDS = new Set<CliCommand>([
  "menu",
  "help",
  "version",
  "install",
  "uninstall",
  "status",
  "doctor",
  "recover",
  "runtime",
  "open",
  "update",
  "self-uninstall",
]);

const SWITCH_FLAGS = new Set([
  "--help",
  "-h",
  "--clone",
  "--live",
  "--yes",
  "--confirm-live",
  "--dry-run",
  "-n",
  "--json",
  "--restore-app",
]);

const VALUE_FLAGS = new Set(["--app", "--transaction"]);

export function parseCli(argv: string[]): ParsedCli {
  const raw = argv[2];
  const flags = argv.slice(3);
  const command = parseCommand(raw);
  rejectUnknownArgs(flags);
  const help = command === "help" || flags.includes("--help") || flags.includes("-h");
  const clone = flags.includes("--clone");
  const liveFlag = flags.includes("--live");
  const yes = flags.includes("--yes") || flags.includes("--confirm-live");
  const dryRun = flags.includes("--dry-run") || flags.includes("-n");
  const json = flags.includes("--json");
  const restoreApp = flags.includes("--restore-app");
  const app = valueAfter(flags, "--app");
  const transaction = valueAfter(flags, "--transaction");

  if (clone && liveFlag) {
    throw new Error("--clone and --live cannot be used together");
  }
  if (clone && app) {
    throw new Error("--clone and --app cannot be used together");
  }
  if (command === "recover" && !help && !transaction) {
    throw new Error("recover requires --transaction <id>\n  incodex recover --transaction <id>");
  }

  const official = !clone && !app;
  const live = (command === "install" || command === "uninstall") && official;
  return { command, help, clone, live, yes, dryRun, json, restoreApp, app, transaction };
}

function rejectUnknownArgs(flags: string[]): void {
  for (let i = 0; i < flags.length; i += 1) {
    const arg = flags[i]!;
    if (VALUE_FLAGS.has(arg)) {
      i += 1;
      continue;
    }
    if (SWITCH_FLAGS.has(arg)) continue;
    if (arg.startsWith("-")) {
      throw new Error(`unknown flag: ${arg}\n  incodex --help`);
    }
    throw new Error(`unexpected argument: ${arg}\n  incodex --help`);
  }
}

function parseCommand(raw: string | undefined): CliCommand {
  if (raw === undefined || raw === "") return "menu";
  if (raw === "-h" || raw === "--help" || raw === "help") return "help";
  if (raw === "-V" || raw === "--version" || raw === "version") return "version";
  if (COMMANDS.has(raw as CliCommand) && raw !== "menu") return raw as CliCommand;
  throw new Error(`unknown command: ${raw}\n  incodex --help`);
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
