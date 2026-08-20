#!/usr/bin/env bun
import { existsSync, unlinkSync } from "node:fs";
import { inspectApp } from "./app-identity";
import { cliVersion } from "./cli-version";
import { collectVersionFacts, formatVersionReport } from "./version-report";
import { verifyApp } from "./codesign";
import { confirmDecision, isTty, requireYesMessage } from "./confirm";
import { askToContinue } from "./confirm-prompt";
import { diagnose, printDiagnosis } from "./doctor";
import { commandHelp, rootHelp } from "./help";
import { detectInstallChannel, selfUninstallPaths, updateAction } from "./cli-channel";
import { formatCommandResult } from "./command-result";
import { formatKv, formatOk, formatStep, formatWarn } from "./cli-print";
import { withSpinner } from "./spinner";
import { cloneOfficialApp, install, installExternalRuntime, listOfficialPids, officialInstallWouldSkip, openOfficialApp, resolveTarget } from "./install";
import { QUIT_PROMPT } from "./quit-official";
import { relaunchDecision } from "./relaunch";
import { runMenu } from "./menu";
import { fetchLatestReleaseTag, readUpdateMessageCache, refreshUpdateNotice, UPDATE_CACHE_PATH } from "./menu-update";
import {
  defaultSourceHome,
  describeIncognitoOpen,
  formatSessionCleanup,
  prepareIncognitoOpen,
  waitAndBurn,
} from "./open-incognito";
import { DEFAULT_APP, USER_ROOT } from "./paths";
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
    console.log(formatVersionReport(collectVersionFacts()));
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
    const choice = await runMenu({ updateMessage: menuUpdateMessage() });
    if (choice === "quit") return;
    if (choice === "version") {
      console.log(formatVersionReport(collectVersionFacts()));
      return;
    }
    await dispatch({
      command: choice,
      help: false,
      clone: false,
      live: true,
      yes: false,
      dryRun: false,
      json: false,
      restoreApp: false,
    });
    return;
  }
  await dispatch(parsed);
}

function menuUpdateMessage(): string | undefined {
  const argv1 = process.argv[1] ?? process.execPath;
  const channel = detectInstallChannel({ execPath: process.execPath, argv1 });
  void refreshUpdateNotice({
    cachePath: UPDATE_CACHE_PATH,
    current: cliVersion(),
    channel,
    fetchLatest: () => fetchLatestReleaseTag(),
  }).catch(() => {});
  return readUpdateMessageCache(UPDATE_CACHE_PATH, argv1) || undefined;
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
    const result = installExternalRuntime();
    console.log(formatStep("Runtime"));
    console.log(formatOk("Runtime updated. Codex was not modified. Reopen it to load the new logic."));
    console.log(formatCommandResult(result));
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
    await runOpen(parsed);
    return;
  }
  if (parsed.command === "update") {
    runUpdate();
    return;
  }
  if (parsed.command === "self-uninstall") {
    await runSelfUninstall(parsed);
  }
}

function runUpdate(): never {
  const channel = detectInstallChannel({ execPath: process.execPath, argv1: process.argv[1] ?? "" });
  const action = updateAction(channel);
  throw new Error(action.message);
}

async function runSelfUninstall(parsed: ParsedCli): Promise<void> {
  const argv1 = process.argv[1] ?? process.execPath;
  const channel = detectInstallChannel({ execPath: process.execPath, argv1 });
  if (channel === "homebrew") {
    throw new Error("this copy was installed with Homebrew\n  brew uninstall incodex");
  }
  if (channel === "source") {
    throw new Error("this copy is running from source\n  bun unlink");
  }
  const paths = selfUninstallPaths(argv1);
  console.log("remove:");
  for (const path of paths) console.log(" ", path);
  if (parsed.restoreApp) console.log("also restore:", DEFAULT_APP);
  if (parsed.dryRun) {
    console.log("no changes made.");
    return;
  }
  await ensureConfirmed("uninstall", parsed);
  if (parsed.restoreApp) {
    uninstall(DEFAULT_APP);
    console.log("restored", DEFAULT_APP);
  }
  for (const path of paths) {
    if (existsSync(path)) unlinkSync(path);
  }
  console.log("done");
}

async function runOpen(parsed: ParsedCli): Promise<void> {
  const appPath = parsed.app ?? DEFAULT_APP;
  if (parsed.dryRun) {
    const described = describeIncognitoOpen({ appPath, userRoot: USER_ROOT });
    console.log(formatStep("Open incognito without patching Codex"));
    console.log(formatKv("App", appPath));
    console.log(formatKv("Binary", described.bin));
    console.log(formatWarn("Dry run. No window opened."));
    return;
  }
  const plan = prepareIncognitoOpen({
    appPath,
    userRoot: USER_ROOT,
    sourceHome: defaultSourceHome(),
  });
  console.log(formatStep("Opening incognito window"));
  console.log(formatKv("Binary", plan.bin));
  console.log(formatKv("Home", plan.home));
  const result = await withSpinner("Waiting for the window to close", () => waitAndBurn(plan, USER_ROOT));
  const closed = formatSessionCleanup(result.cleanup);
  console.log(closed.ok ? formatOk(closed.message) : formatWarn(closed.message));
  console.log("");
}

async function runInstall(parsed: ParsedCli, appPath: string): Promise<void> {
  if (parsed.clone && !parsed.app) {
    console.log(formatKv("Clone", appPath));
  }
  printInstallPlan(appPath, parsed.clone);
  if (parsed.dryRun) {
    console.log(formatWarn("Dry run. No files changed."));
    return;
  }
  const skip = !parsed.clone && officialInstallWouldSkip(appPath);
  if (!skip) await ensureConfirmed("install", parsed);
  if (parsed.clone && !parsed.app) {
    cloneOfficialApp(appPath);
    console.log(formatOk("Cloned official app"));
    console.log(formatKv("Target", appPath));
  }
  const pidsBefore = listOfficialPids();
  if (!skip && parsed.live && !parsed.app && pidsBefore.length > 0) {
    await ensureQuitConfirmed(parsed);
  }
  const result = await install(appPath);
  console.log(formatCommandResult(result));
  if (parsed.live && !parsed.app) {
    await maybeRelaunchChatGPT(pidsBefore, result.skipped === true);
  } else {
    console.log(formatOk("Restart that app copy to see the Incognito button."));
  }
  console.log("");
}

async function runUninstall(parsed: ParsedCli, appPath: string): Promise<void> {
  const target = parsed.app ?? appPath;
  console.log(formatStep("Uninstall"));
  console.log(formatKv("App", target));
  if (parsed.dryRun) {
    console.log(formatWarn("Dry run. No files changed."));
    return;
  }
  await ensureConfirmed("uninstall", parsed);
  const result = uninstall(target);
  console.log(formatOk("Official app restored. Dock was refreshed."));
  console.log(formatCommandResult(result));
  console.log("");
}

function printInstallPlan(appPath: string, clone: boolean): void {
  const source = clone ? DEFAULT_APP : appPath;
  const info = inspectApp(source);
  console.log(formatStep(clone ? "Clone install" : "Install"));
  console.log(formatKv("App", clone ? appPath : source));
  if (clone) console.log(formatKv("Source", source));
  console.log(formatKv("Version", `${info.listing?.appVersion ?? "unknown"} ${info.listing?.appBuild ?? ""}`.trim()));
  console.log(formatKv("Signed", verifyApp(source) ? "yes" : "no"));
  if (!clone && officialInstallWouldSkip(appPath)) return;
  if (!clone) {
    console.log(formatWarn("Replaces the app in place and resigns it ad hoc."));
    console.log(formatWarn("Official Appshot (smart snapshot) stops until uninstall."));
    console.log(formatKv("Backup", "~/.incodex/installations/"));
    if (listOfficialPids().length > 0) {
      console.log(formatWarn("ChatGPT is running. Install will quit it."));
    }
  }
}

async function ensureQuitConfirmed(parsed: ParsedCli): Promise<void> {
  const decision = confirmDecision({
    clone: false,
    dryRun: false,
    yes: parsed.yes,
    tty: isTty(),
  });
  if (decision === "ok") return;
  if (decision === "require-yes") {
    throw new Error(requireYesMessage("install"));
  }
  const ok = await askToContinue(QUIT_PROMPT);
  if (!ok) throw new Error("aborted");
}

async function maybeRelaunchChatGPT(pidsBefore: number[], skipped: boolean): Promise<void> {
  const action = relaunchDecision({
    before: pidsBefore,
    after: listOfficialPids(),
    skipped,
  });
  if (action === "none") {
    if (!skipped && pidsBefore.length === 0) {
      console.log(formatOk("Done. Open ChatGPT.app when you want Incognito."));
    }
    return;
  }
  console.log(formatStep("Relaunch"));
  openOfficialApp();
  console.log(formatOk("ChatGPT.app relaunched."));
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
