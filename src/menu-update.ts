import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import type { InstallChannel } from "./cli-channel";
import { USER_ROOT } from "./paths";

export const UPDATE_CACHE_PATH = join(USER_ROOT, "cache", "update_message");
export const LATEST_RELEASE_URL = "https://api.github.com/repos/daftAI2026/incodex/releases/latest";

export function formatUpdateNotice(latest: string): string {
  return `Update ${latest} available, run incodex update`;
}

export function isNewerVersion(latest: string, current: string): boolean {
  const left = parts(latest);
  const right = parts(current);
  const n = Math.max(left.length, right.length);
  for (let i = 0; i < n; i += 1) {
    const a = left[i] ?? 0;
    const b = right[i] ?? 0;
    if (a > b) return true;
    if (a < b) return false;
  }
  return false;
}

export function buildUpdateNotice(input: {
  channel: InstallChannel;
  current: string;
  latest: string | undefined;
}): string | undefined {
  if (input.channel !== "script") return undefined;
  if (!input.latest) return undefined;
  if (!isNewerVersion(input.latest, input.current)) return undefined;
  return formatUpdateNotice(input.latest);
}

export function writeUpdateMessageCache(cachePath: string, message: string): void {
  mkdirSync(dirname(cachePath), { recursive: true });
  writeFileSync(cachePath, message);
}

export function readUpdateMessageCache(cachePath: string, binaryPath: string): string {
  if (!existsSync(cachePath)) return "";
  if (existsSync(binaryPath)) {
    const cacheMtime = statSync(cachePath).mtimeMs;
    const binaryMtime = statSync(binaryPath).mtimeMs;
    if (cacheMtime < binaryMtime) {
      writeUpdateMessageCache(cachePath, "");
      return "";
    }
  }
  return readFileSync(cachePath, "utf8").trim();
}

export type FetchLike = (url: string, init?: RequestInit) => Promise<Response>;

export async function fetchLatestReleaseTag(input: {
  fetchImpl?: FetchLike;
  url?: string;
  timeoutMs?: number;
} = {}): Promise<string | undefined> {
  const fetchImpl = input.fetchImpl ?? fetch;
  try {
    const response = await fetchImpl(input.url ?? LATEST_RELEASE_URL, {
      signal: AbortSignal.timeout(input.timeoutMs ?? 3000),
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!response.ok) return undefined;
    const body = (await response.json()) as { tag_name?: string };
    const tag = body.tag_name?.trim().replace(/^v/i, "");
    return tag || undefined;
  } catch {
    return undefined;
  }
}

export async function refreshUpdateNotice(input: {
  cachePath: string;
  current: string;
  channel: InstallChannel;
  fetchLatest: () => Promise<string | undefined>;
}): Promise<string> {
  const notice = buildUpdateNotice({
    channel: input.channel,
    current: input.current,
    latest: await input.fetchLatest(),
  });
  writeUpdateMessageCache(input.cachePath, notice ?? "");
  return notice ?? "";
}

function parts(version: string): number[] {
  return version
    .replace(/^v/i, "")
    .split(".")
    .map((piece) => {
      const n = Number(piece);
      return Number.isFinite(n) ? n : 0;
    });
}
