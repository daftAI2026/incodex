import { readdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repositoryRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const WINDOWS_TEST_CRATES = ["incodex-core", "incodex-cli"] as const;

function windowsTestSuites(root: string, crate: string): string[] {
  return readdirSync(join(root, "crates", crate, "tests"))
    .filter((name) => name.startsWith("windows_") && name.endsWith(".rs"))
    .map((name) => name.slice(0, -3))
    .sort();
}

export function windowsRustCommands(root = repositoryRoot): string[][] {
  const commands = [
    ["cargo", "fmt", "--all", "--", "--check"],
    ["cargo", "clippy", "-p", "incodex-core", "--lib", "--locked", "--", "-D", "warnings"],
    [
      "cargo",
      "clippy",
      "-p",
      "incodex-cli",
      "--lib",
      "--bin",
      "incodex",
      "--locked",
      "--",
      "-D",
      "warnings",
    ],
  ];

  for (const crate of WINDOWS_TEST_CRATES) {
    commands.push(["cargo", "test", "-p", crate, "--lib", "--release", "--locked"]);
    for (const suite of windowsTestSuites(root, crate)) {
      commands.push([
        "cargo",
        "test",
        "-p",
        crate,
        "--test",
        suite,
        "--release",
        "--locked",
      ]);
    }
  }

  return commands;
}

function main(): void {
  for (const [command, ...args] of windowsRustCommands()) {
    const result = spawnSync(command, args, {
      cwd: repositoryRoot,
      stdio: "inherit",
    });
    if (result.status !== 0) {
      process.exit(result.status ?? 1);
    }
  }
}

if (import.meta.main) {
  main();
}
