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
  copySettings(session.home, options.sourceHome, options.userRoot);
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

export async function waitAndBurn(
  plan: IncognitoOpenPlan,
  userRoot: string,
  spawnImpl: typeof spawn = spawn,
): Promise<number> {
  const child = spawnImpl(plan.bin, plan.args, {
    env: { ...process.env, ...plan.env },
    stdio: "ignore",
  });
  const code: number = await new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("exit", (status) => resolve(status ?? 1));
  });
  await burnWithRetries(plan.sessionRoot, { userRoot, sessionId: plan.sessionId });
  return code;
}

async function burnWithRetries(
  sessionRoot: string,
  expected: { userRoot: string; sessionId: string },
): Promise<void> {
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      burnSessionHome(sessionRoot, expected);
    } catch {
      if (attempt === 4) return;
    }
    if (!existsSync(sessionRoot)) return;
    await new Promise((resolve) => setTimeout(resolve, 250 * (attempt + 1)));
  }
}
