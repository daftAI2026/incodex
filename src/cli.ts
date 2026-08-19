#!/usr/bin/env bun
import { inspectApp } from "./app-identity";
import { cliVersion } from "./cli-version";
import { verifyApp } from "./codesign";
import { confirmDecision, isTty, requireYesMessage } from "./confirm";
import { askToContinue } from "./confirm-prompt";
import { diagnose, printDiagnosis } from "./doctor";
import { commandHelp, rootHelp } from "./help";
import { cloneOfficialApp, install, installExternalRuntime, resolveTarget } from "./install";
import { runMenu } from "./menu";
import { DEFAULT_APP } from "./paths";
import { parseCli, type ParsedCli } from "./parse-cli";
import { recoverTransaction } from "./recover";
import { printStatus } from "./status";
import { uninstall } from "./uninstall";

try {
  await main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}

async function main(): Promise<void> {
  const parsed = parseCli(process.argv);
  if (parsed.command === "version") {
    console.log(cliVersion());
    return;
  }
  if (parsed.command === "help" || parsed.help) {
    console.log(parsed.command === "help" || parsed.command === "menu" ? rootHelp() : commandHelp(parsed.command));
    return;
  }
  if (parsed.command === "menu") {
    if (!isTty()) {
      console.log(rootHelp());
      return;
    }
    const choice = await runMenu();
    if (choice === "quit") return;
    await dispatch({
      command: choice,
      help: false,
      clone: false,
      live: true,
      yes: false,
      dryRun: false,
      json: false,
    });
    return;
  }
  await dispatch(parsed);
}

async function dispatch(parsed: ParsedCli): Promise<void> {
  const appPath = resolveTarget({
    clone: parsed.clone,
    live: parsed.live,
    app: parsed.app,
  });

  if (parsed.command === "install") {
    await runInstall(parsed, appPath);
    return;
  }
  if (parsed.command === "uninstall") {
    await runUninstall(parsed, appPath);
    return;
  }
  if (parsed.command === "status") {
    const target = parsed.app ?? DEFAULT_APP;
    if (parsed.json) {
      console.log(JSON.stringify(diagnose(target), null, 2));
    } else {
      printStatus(target);
    }
    return;
  }
  if (parsed.command === "doctor") {
    const target = parsed.app ?? DEFAULT_APP;
    if (parsed.json) {
      console.log(JSON.stringify(diagnose(target), null, 2));
    } else {
      printDiagnosis(diagnose(target));
    }
    return;
  }
  if (parsed.command === "runtime") {
    if (parsed.dryRun) {
      console.log("would update ~/.incodex/runtime/ without modifying Codex");
      return;
    }
    installExternalRuntime();
    console.log("done. Codex was not modified. reopen it to load the new runtime.");
    return;
  }
  if (parsed.command === "recover") {
    const result = recoverTransaction(parsed.transaction!);
    console.log("phase:", result.journal.phase);
    console.log("action:", result.action);
    console.log("target:", result.journal.targetRealPath);
    console.log("target present:", result.targetUntouched);
    console.log("backup intact:", result.backupIntact);
    console.log("staged removed:", result.stagedRemoved);
    console.log("outgoing restored:", result.outgoingRestored);
    return;
  }
  if (parsed.command === "open") {
    throw new Error(
      "incodex open is not in this release.\n  After install, use the hat button or Shift+Command+N.",
    );
  }
}

async function runInstall(parsed: ParsedCli, appPath: string): Promise<void> {
  if (parsed.clone && !parsed.app) {
    console.log("clone target:", appPath);
  }
  printInstallPlan(appPath, parsed.clone);
  if (parsed.dryRun) {
    console.log("no changes made.");
    return;
  }
  await ensureConfirmed("install", parsed);
  if (parsed.clone && !parsed.app) {
    console.log("cloning official app to", appPath);
    cloneOfficialApp(appPath);
  }
  console.log("installing into", appPath);
  await install(appPath);
  if (parsed.live && !parsed.app) {
    console.log("done. reopen /Applications/ChatGPT.app to use Incognito.");
  } else {
    console.log("done. restart that app copy to see the Incognito button.");
  }
}

async function runUninstall(parsed: ParsedCli, appPath: string): Promise<void> {
  const target = parsed.app ?? appPath;
  console.log("restore target:", target);
  if (parsed.dryRun) {
    console.log("no changes made.");
    return;
  }
  await ensureConfirmed("uninstall", parsed);
  console.log("restoring", target);
  uninstall(target);
  console.log("done");
}

function printInstallPlan(appPath: string, clone: boolean): void {
  const source = clone ? DEFAULT_APP : appPath;
  const info = inspectApp(source);
  console.log(clone ? "planned clone install" : "planned install");
  console.log("  app:", clone ? appPath : source);
  if (clone) console.log("  source:", source);
  console.log("  version:", info.listing?.appVersion ?? "unknown", info.listing?.appBuild ?? "");
  console.log("  signed:", verifyApp(source));
  if (!clone) {
    console.log("  this replaces the app in place and resigns it ad hoc");
    console.log("  official Appshot (smart snapshot) will stop working until uninstall");
    console.log("  backup will be written under ~/.incodex/installations/");
  }
}

async function ensureConfirmed(command: "install" | "uninstall", parsed: ParsedCli): Promise<void> {
  const decision = confirmDecision({
    clone: parsed.clone,
    dryRun: parsed.dryRun,
    yes: parsed.yes,
    tty: isTty(),
  });
  if (decision === "ok") return;
  if (decision === "require-yes") {
    throw new Error(requireYesMessage(command));
  }
  const ok = await askToContinue();
  if (!ok) throw new Error("aborted");
}
