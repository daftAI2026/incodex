import { spawnSync } from "node:child_process";
import { detectInstallChannel, type InstallChannel } from "./cli-channel";
import { cliVersion } from "./cli-version";

export type VersionFacts = {
  version: string;
  macos: string;
  architecture: string;
  kernel: string;
  sip: string;
  diskFree: string;
  install: string;
  shell: string;
};

const CHANNEL_LABEL: Record<InstallChannel, string> = {
  source: "Source",
  script: "Script",
  homebrew: "Homebrew",
};

export function formatVersionReport(facts: VersionFacts): string {
  return [
    `Incodex version ${facts.version}`,
    `macOS: ${facts.macos}`,
    `Architecture: ${facts.architecture}`,
    `Kernel: ${facts.kernel}`,
    `SIP: ${facts.sip}`,
    `Disk Free: ${facts.diskFree}`,
    `Install: ${facts.install}`,
    `Shell: ${facts.shell}`,
    "",
  ].join("\n");
}

export function collectVersionFacts(input: { execPath: string; argv1: string; env?: NodeJS.Dict<string> } = {
  execPath: process.execPath,
  argv1: process.argv[1] ?? "",
  env: process.env,
}): VersionFacts {
  const env = input.env ?? process.env;
  const channel = detectInstallChannel({ execPath: input.execPath, argv1: input.argv1 });
  return {
    version: cliVersion(),
    macos: probe("sw_vers", ["-productVersion"]) || "Unknown",
    architecture: probe("uname", ["-m"]) || "Unknown",
    kernel: probe("uname", ["-r"]) || "Unknown",
    sip: sipStatus(),
    diskFree: diskFree(),
    install: CHANNEL_LABEL[channel],
    shell: env.SHELL || "Unknown",
  };
}

function probe(cmd: string, args: string[]): string {
  const ran = spawnSync(cmd, args, { encoding: "utf8" });
  if (ran.status !== 0) return "";
  return (ran.stdout || "").trim();
}

function sipStatus(): string {
  const raw = probe("csrutil", ["status"]).toLowerCase();
  if (raw.includes("enabled")) return "Enabled";
  if (raw.includes("disabled")) return "Disabled";
  return "Unknown";
}

function diskFree(): string {
  const raw = probe("df", ["-k", "/"]);
  const lines = raw.split("\n");
  const data = lines[1];
  if (!data) return "Unknown";
  const cols = data.trim().split(/\s+/);
  const availKb = Number(cols[3]);
  if (!Number.isFinite(availKb) || availKb < 0) return "Unknown";
  return `${(availKb / 1024 / 1024).toFixed(2)}GB`;
}
