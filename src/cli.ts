#!/usr/bin/env bun
import { cloneOfficialApp, install, resolveTarget } from "./install";
import { inspectApp } from "./app-identity";
import { parseCli } from "./parse-cli";
import { DEFAULT_APP } from "./paths";
import { diagnose, printDiagnosis } from "./doctor";
import { printStatus } from "./status";
import { loadJournal, recoverAction } from "./transaction";
import { uninstall } from "./uninstall";
import { verifyApp } from "./codesign";

try {
const parsed = parseCli(process.argv);

if (parsed.command === "help") {
  console.log(`incodex — Incognito toggle for Codex desktop

Usage:
  bun src/cli.ts install --clone
  bun src/cli.ts install --live --confirm-live
  bun src/cli.ts uninstall --live
  bun src/cli.ts uninstall --clone
  bun src/cli.ts uninstall --app <path>
  bun src/cli.ts status [--json] [--app <path>]
  bun src/cli.ts doctor [--app <path>]
  bun src/cli.ts recover --transaction <id>
`);
  process.exit(0);
}

const appPath = resolveTarget({
  clone: parsed.clone,
  live: parsed.live,
  app: parsed.app,
});

  if (parsed.command === "install") {
    if (parsed.clone && !parsed.app) {
      console.log("cloning official app to", appPath);
      cloneOfficialApp(appPath);
    }
    if (parsed.live) {
      const info = inspectApp(DEFAULT_APP);
      console.log("planned live install");
      console.log("  app:", DEFAULT_APP);
      console.log("  version:", info.listing?.appVersion ?? "unknown", info.listing?.appBuild ?? "");
      console.log("  signed:", verifyApp(DEFAULT_APP));
      console.log("  backup will be written under ~/.incodex/installations/");
    }
    console.log("installing into", appPath);
    await install(appPath);
    if (parsed.live && !parsed.app) {
      console.log("done. reopen /Applications/ChatGPT.app to use Incognito.");
    } else {
      console.log("done. restart that app copy to see the Incognito button.");
    }
  } else if (parsed.command === "uninstall") {
    const target = parsed.app ?? (parsed.live ? DEFAULT_APP : appPath);
    console.log("restoring", target);
    uninstall(target);
    console.log("done");
  } else if (parsed.command === "status") {
    const target = parsed.app ?? DEFAULT_APP;
    if (parsed.json) {
      console.log(JSON.stringify(diagnose(target), null, 2));
    } else {
      printStatus(target);
    }
  } else if (parsed.command === "doctor") {
    printDiagnosis(diagnose(parsed.app ?? DEFAULT_APP));
  } else if (parsed.command === "recover") {
    const journal = loadJournal(parsed.transaction!);
    if (!journal) throw new Error(`no journal for ${parsed.transaction}`);
    console.log("phase:", journal.phase);
    console.log("action:", recoverAction(journal));
    console.log("target:", journal.targetRealPath);
    console.log("staged:", journal.stagedApp);
    console.log("original:", journal.originalSnapshot);
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
