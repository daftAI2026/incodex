import { randomBytes } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  burnSessionHome,
  createSessionHome,
  sweepOrphanSessions,
} from "./runtime/incodex-safe-home.cts";

export const EXIT_PATHS = [
  "close",
  "click-exit",
  "cmd-q",
  "crash",
  "sigterm",
  "sigkill",
  "power-off",
  "janitor",
] as const;

export type ExitPath = (typeof EXIT_PATHS)[number];

export type SessionDirs = {
  sessionId: string;
  root: string;
  home: string;
  chromium: string;
  ino: number;
  dev: number;
};

export type ForensicHit = {
  path: string;
  kind: "name" | "content";
};

export type ScanRoots = {
  userRoot: string;
  sourceHome: string;
  tmp: string;
  crashDumps: string;
  applicationSupport: string;
  caches: string;
  savedState: string;
};

export function uniquePrompt(): string {
  return `INCODEX-FORENSICS-${randomBytes(16).toString("hex")}`;
}

export function createForensicSession(userRoot: string, sourceHome: string, pid = 0): SessionDirs {
  return createSessionHome(userRoot, {
    targetId: "forensics",
    pid,
    sourceHome,
  }) as SessionDirs;
}

export function plantPrompt(session: SessionDirs, sandbox: ScanRoots, prompt: string, ambient = false): string[] {
  const planted: string[] = [];

  function write(file: string): void {
    mkdirSync(join(file, ".."), { recursive: true });
    writeFileSync(file, `${prompt}\n`);
    planted.push(file);
  }

  write(join(session.home, "sessions", "secret-chat.json"));
  write(join(session.chromium, "Default", "Cache", "prompt.bin"));
  write(join(session.chromium, "Default", "Local Storage", "leveldb", "000003.log"));
  if (ambient) {
    write(join(sandbox.tmp, "incodex-session-tmp.txt"));
    write(join(sandbox.crashDumps, "ChatGPT_crash.txt"));
    write(join(sandbox.applicationSupport, "com.openai.codex", "cache.db"));
    write(join(sandbox.caches, "com.openai.codex", "fsCachedData"));
    write(join(sandbox.savedState, "com.openai.codex.savedState", "windows.plist"));
  }
  return planted;
}

export function handleExit(kind: ExitPath, session: SessionDirs, userRoot: string): void {
  function burn(): void {
    burnSessionHome(session.root, {
      userRoot,
      sessionId: session.sessionId,
      ino: session.ino,
      dev: session.dev,
    });
  }

  switch (kind) {
    case "close":
    case "click-exit":
    case "cmd-q":
    case "crash":
    case "sigterm":
      burn();
      return;
    case "sigkill":
    case "power-off":
      return;
    case "janitor":
      sweepOrphanSessions(userRoot);
  }
}

export function scanForPrompt(roots: string[], prompt: string): ForensicHit[] {
  const hits: ForensicHit[] = [];
  for (const root of roots) walk(root, prompt, hits);
  return hits;
}

export function sessionEvidenceGone(session: SessionDirs, prompt: string): boolean {
  if (existsSync(session.root)) {
    return scanForPrompt([session.root], prompt).length === 0;
  }
  return true;
}

export function absolutePrivacyClaimAllowed(): boolean {
  return false;
}

function walk(root: string, prompt: string, hits: ForensicHit[]): void {
  if (!existsSync(root)) return;
  const stats = statSync(root);
  if (stats.isSymbolicLink()) return;
  if (root.includes(prompt)) hits.push({ path: root, kind: "name" });
  if (stats.isFile()) {
    try {
      if (readFileSync(root).includes(prompt)) hits.push({ path: root, kind: "content" });
    } catch {
      /* unreadable files are not treated as a hit */
    }
    return;
  }
  if (!stats.isDirectory()) return;
  for (const name of readdirSync(root)) {
    walk(join(root, name), prompt, hits);
  }
}
