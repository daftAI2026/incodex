import { spawnSync } from "node:child_process";
import { formatKv, formatOk } from "./cli-print";
import { DEFAULT_APP } from "./paths";

export const QUIT_PROMPT = "ChatGPT is running. Press Enter to quit it, ESC to abort: ";
export const RELAUNCH_PROMPT = "Press Enter to relaunch ChatGPT, ESC to leave it running: ";
export const STILL_RUNNING_MESSAGE = "ChatGPT is still running. Install aborted.";
export const QUIT_APPLESCRIPT = 'tell application id "com.openai.codex" to quit';

export function listOfficialPids(): number[] {
  const listed = spawnSync("ps", ["-ax", "-o", "pid=,command="], { encoding: "utf8" });
  const needle = `${DEFAULT_APP}/Contents/MacOS/ChatGPT`;
  return (listed.stdout || "")
    .split("\n")
    .filter((line) => line.includes(needle))
    .map((line) => Number(line.trim().split(/\s+/)[0]))
    .filter((pid) => Number.isInteger(pid) && pid > 0);
}

export type QuitOutcome = "gone" | "still-running";

export function quitOutcome(pidsAfter: number[]): QuitOutcome {
  return pidsAfter.length === 0 ? "gone" : "still-running";
}

export function requestAppleQuit(spawn: typeof spawnSync = spawnSync): { status: number | null; stderr: string } {
  const ran = spawn("osascript", ["-e", QUIT_APPLESCRIPT], { encoding: "utf8" });
  return { status: ran.status, stderr: ran.stderr || "" };
}

export function waitUntilOfficialGone(options: {
  listPids?: () => number[];
  sleepMs?: (ms: number) => void;
  now?: () => number;
  timeoutMs?: number;
  intervalMs?: number;
} = {}): boolean {
  const listPids = options.listPids ?? listOfficialPids;
  const sleepMs = options.sleepMs ?? ((ms: number) => spawnSync("sleep", [String(ms / 1000)]));
  const now = options.now ?? Date.now;
  const timeoutMs = options.timeoutMs ?? 60_000;
  const intervalMs = options.intervalMs ?? 200;
  const deadline = now() + timeoutMs;
  while (listPids().length > 0) {
    if (now() >= deadline) return false;
    sleepMs(intervalMs);
  }
  return true;
}

export function quitOfficialApp(): void {
  const pids = listOfficialPids();
  if (pids.length === 0) return;
  console.log(formatOk("Quitting Codex"));
  console.log(formatKv("Pids", pids.join(" ")));
  requestAppleQuit();
  if (!waitUntilOfficialGone()) {
    throw new Error(STILL_RUNNING_MESSAGE);
  }
  console.log(formatOk("Quit Codex"));
}
