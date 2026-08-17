#!/usr/bin/env bun
import { cloneOfficialApp, install, resolveTarget } from "./install";
import { printStatus } from "./status";
import { uninstall } from "./uninstall";
import { DEFAULT_APP } from "./paths";

function arg(name: string): string | undefined {
  const index = process.argv.indexOf(name);
  if (index === -1) return undefined;
  return process.argv[index + 1];
}

function has(flag: string): boolean {
  return process.argv.includes(flag);
}

const command = process.argv[2] ?? "help";

if (command === "help" || command === "-h" || command === "--help") {
  console.log(`incodex — Incognito toggle for Codex desktop

Usage:
  bun src/cli.ts install --clone    patch a copy only (safe)
  bun src/cli.ts install --live     patch ${DEFAULT_APP} after backup
  bun src/cli.ts uninstall --live   restore the backup
  bun src/cli.ts status
`);
  process.exit(0);
}

const appPath = resolveTarget({
  clone: has("--clone"),
  live: has("--live"),
  app: arg("--app"),
});

try {
  if (command === "install") {
    if (!has("--clone") && !has("--live") && !arg("--app")) {
      throw new Error("pass --clone (safe copy) or --live (official app)");
    }
    if (has("--clone") && !arg("--app")) {
      console.log("cloning official app to", appPath);
      cloneOfficialApp(appPath);
    }
    console.log("installing into", appPath);
    await install(appPath);
    console.log("done. restart that app copy to see the Incognito button.");
  } else if (command === "uninstall") {
    const target = arg("--app") ?? (has("--live") ? DEFAULT_APP : appPath);
    console.log("restoring", target);
    uninstall(target);
    console.log("done");
  } else if (command === "status") {
    printStatus(arg("--app") ?? DEFAULT_APP);
  } else {
    throw new Error(`unknown command: ${command}`);
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
