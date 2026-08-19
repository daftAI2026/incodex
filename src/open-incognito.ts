import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { targetIdFromExec } from "./runtime/incodex-instance.cts";
import { burnSessionHome, copySettings, createSessionHome, sweepOrphanSessions } from "./runtime/incodex-safe-home.cts";

export type IncognitoOpenDescription = {
  bin: string;
  args: string[];
  appPath: string;
};

export type IncognitoOpenPlan = {
  bin: string;
  args: string[];
  env: Record<string, string>;
  home: string;
  chromium: string;
  sessionId: string;
  sessionRoot: string;
};

export type CleanupResult =
  | { removed: true; attempts: number }
  | { removed: false; attempts: number; retainedPath: string; reason: string };

export type WaitAndBurnResult = {
  code: number;
  cleanup: CleanupResult;
};

export function chatGptBinary(appPath: string): string {
  return join(appPath, "Contents/MacOS/ChatGPT");
}

export function describeIncognitoOpen(options: { appPath: string; userRoot: string }): IncognitoOpenDescription {
  const bin = chatGptBinary(options.appPath);
  return {
    bin,
    args: ["--user-data-dir=<isolated-chromium>"],
    appPath: options.appPath,
  };
}

export function prepareIncognitoOpen(options: {
  appPath: string;
  userRoot: string;
  sourceHome: string;
  pid?: number;
  copySettings?: typeof copySettings;
  burnSessionHome?: typeof burnSessionHome;
}): IncognitoOpenPlan {
  const bin = chatGptBinary(options.appPath);
  if (!existsSync(bin)) throw new Error(`Codex binary not found: ${bin}`);
  const targetId = targetIdFromExec(bin);
  try {
    sweepOrphanSessions(options.userRoot, { targetId });
  } catch {
    /* janitor best-effort */
  }
  const session = createSessionHome(options.userRoot, {
    targetId,
    pid: options.pid ?? process.pid,
    sourceHome: options.sourceHome,
  });
  try {
    (options.copySettings ?? copySettings)(session.home, options.sourceHome, options.userRoot);
  } catch (error) {
    try {
      (options.burnSessionHome ?? burnSessionHome)(session.root, {
        userRoot: options.userRoot,
        sessionId: session.sessionId,
      });
    } catch {
      /* still throw the copy failure */
    }
    throw error;
  }
  return {
    bin,
    args: [`--user-data-dir=${session.chromium}`],
    env: {
      CODEX_HOME: session.home,
      INCODEX_INCOGNITO: "1",
      INCODEX_SESSION_ID: session.sessionId,
      INCODEX_SESSION_ROOT: session.root,
      CODEX_ELECTRON_USER_DATA_PATH: session.chromium,
      INCODEX_SOURCE_HOME: options.sourceHome,
    },
    home: session.home,
    chromium: session.chromium,
    sessionId: session.sessionId,
    sessionRoot: session.root,
  };
}

export function defaultSourceHome(env: NodeJS.Dict<string> = process.env): string {
  return env.CODEX_HOME || join(homedir(), ".codex");
}

export function formatSessionCleanup(cleanup: CleanupResult): { ok: boolean; message: string } {
  if (cleanup.removed) {
    return { ok: true, message: "Closed. Isolated session removed." };
  }
  return {
    ok: false,
    message: `Closed. Isolated session kept at ${cleanup.retainedPath} (${cleanup.reason})`,
  };
}

export async function waitAndBurn(
  plan: IncognitoOpenPlan,
  userRoot: string,
  spawnImpl: typeof spawn = spawn,
  options: {
    retryDelayMs?: number;
    burn?: typeof burnSessionHome;
  } = {},
): Promise<WaitAndBurnResult> {
  const child = spawnImpl(plan.bin, plan.args, {
    env: { ...process.env, ...plan.env },
    stdio: "ignore",
  });
  let code = 1;
  try {
    code = await new Promise((resolve, reject) => {
      child.on("error", reject);
      child.on("exit", (status) => resolve(status ?? 1));
    });
  } catch {
    code = 1;
  }
  const cleanup = await burnWithRetries(plan.sessionRoot, { userRoot, sessionId: plan.sessionId }, options);
  return { code, cleanup };
}

async function burnWithRetries(
  sessionRoot: string,
  expected: { userRoot: string; sessionId: string },
  options: { retryDelayMs?: number; burn?: typeof burnSessionHome } = {},
): Promise<CleanupResult> {
  const delay = options.retryDelayMs ?? 250;
  const burn = options.burn ?? burnSessionHome;
  let reason = "session directory still present";
  for (let attempt = 1; attempt <= 5; attempt++) {
    try {
      burn(sessionRoot, expected);
    } catch (error) {
      reason = error instanceof Error ? error.message : String(error);
      if (attempt === 5) {
        return existsSync(sessionRoot)
          ? { removed: false, attempts: attempt, retainedPath: sessionRoot, reason }
          : { removed: true, attempts: attempt };
      }
    }
    if (!existsSync(sessionRoot)) return { removed: true, attempts: attempt };
    if (attempt < 5) await new Promise((resolve) => setTimeout(resolve, delay * attempt));
  }
  return existsSync(sessionRoot)
    ? { removed: false, attempts: 5, retainedPath: sessionRoot, reason }
    : { removed: true, attempts: 5 };
}
