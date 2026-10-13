/**
 * [INPUT]: 依赖目标会话根、无主旧会话目录和进程命令快照。
 * [OUTPUT]: 提供只读 ownerless 基线摘要与稳定性校验。
 * [POS]: A→B→A 验收的历史会话边界；不创建、修复或销毁生产会话。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { createHash } from "node:crypto";
import { closeSync, constants as fsConstants, fstatSync, lstatSync, openSync, readSync, readdirSync } from "node:fs";
import type { Stats } from "node:fs";
import { isAbsolute, join } from "node:path";
import { spawnSync } from "node:child_process";
import { isRecord, SESSION_ID } from "./incodex-macos-runtime-hot-acceptance-contract.ts";

export type SessionBaseline = ReadonlyMap<string, string>;
export type SessionBaselineSummary = { count: number; digest: string };

const MAX_BASELINE_SESSIONS = 128;
const MAX_BASELINE_ENTRIES = 100_000;
const MAX_BASELINE_DEPTH = 48;
const MAX_BASELINE_BYTES = 2 * 1024 * 1024 * 1024;
const MAX_SESSION_FILE_BYTES = 1024 * 1024 * 1024;
const READ_CHUNK_BYTES = 1024 * 1024;

type ScanBudget = { entries: number; bytes: number };

function existingRealDirectory(path: string, label: string): boolean {
  let stat: Stats;
  try { stat = lstatSync(path); }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return false;
    throw new Error(`${label} cannot be inspected safely`);
  }
  if (stat.isSymbolicLink() || !stat.isDirectory()) throw new Error(`${label} is not a real directory`);
  return true;
}

export function targetSessionRootAt(userRoot: string, targetId: string): string | null {
  if (!isAbsolute(userRoot) || !/^[a-f0-9]{12}$/u.test(targetId)) throw new Error("target session identity is invalid");
  if (!existingRealDirectory(userRoot, "Incodex user root")) return null;
  const sessionsRoot = join(userRoot, "sessions");
  if (!existingRealDirectory(sessionsRoot, "session store")) return null;
  const targetRoot = join(sessionsRoot, targetId);
  return existingRealDirectory(targetRoot, "target session root") ? targetRoot : null;
}

function requirePrivateOwned(stat: Stats, kind: string): void {
  if ((typeof process.getuid === "function" && stat.uid !== process.getuid()) || (stat.mode & 0o077) !== 0) {
    throw new Error(`target session ${kind} is not private to the current user`);
  }
}

function readProcessList(): string {
  const result = spawnSync("/bin/ps", ["axEww", "-o", "pid=,command="], {
    encoding: "utf8",
    timeout: 10_000,
    maxBuffer: 4 * 1024 * 1024,
    env: { ...process.env, LC_ALL: "C" },
  });
  if (result.error || result.status !== 0 || typeof result.stdout !== "string") {
    throw new Error("session process references cannot be checked safely");
  }
  return result.stdout;
}

function requireNoProcessReference(sessionRoot: string, processList: string): void {
  if (processList.includes(sessionRoot)) throw new Error("target session process reference blocks acceptance");
}

function requireAbsent(path: string, label: string): void {
  try {
    lstatSync(path);
    throw new Error(`${label} blocks acceptance`);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return;
    throw error;
  }
}

function samePathIdentity(path: string, expected: Stats): void {
  const current = lstatSync(path);
  if (current.isSymbolicLink() || current.dev !== expected.dev || current.ino !== expected.ino ||
      current.mode !== expected.mode || current.size !== expected.size) {
    throw new Error("target session changed during read-only baseline scan");
  }
}

function digestRegularFile(path: string, relative: string, stat: Stats, hash: ReturnType<typeof createHash>, budget: ScanBudget): void {
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > MAX_SESSION_FILE_BYTES) {
    throw new Error("target session contains an unsupported file type");
  }
  requirePrivateOwned(stat, "file");
  if (typeof fsConstants.O_NOFOLLOW !== "number") throw new Error("session files cannot be read without symlink protection");
  const fd = openSync(path, fsConstants.O_RDONLY | fsConstants.O_NOFOLLOW);
  try {
    const opened = fstatSync(fd);
    if (!opened.isFile() || opened.dev !== stat.dev || opened.ino !== stat.ino || opened.mode !== stat.mode ||
        opened.size !== stat.size || opened.size > MAX_SESSION_FILE_BYTES) {
      throw new Error("target session file changed during read-only baseline scan");
    }
    hash.update(`F\0${relative}\0${opened.dev}\0${opened.ino}\0${opened.mode}\0${opened.size}\0`);
    const chunk = Buffer.allocUnsafe(READ_CHUNK_BYTES);
    let offset = 0;
    while (offset < opened.size) {
      const requested = Math.min(chunk.length, opened.size - offset);
      const bytesRead = readSync(fd, chunk, 0, requested, offset);
      if (bytesRead <= 0) throw new Error("target session file ended during baseline scan");
      hash.update(chunk.subarray(0, bytesRead));
      offset += bytesRead;
      budget.bytes += bytesRead;
      if (budget.bytes > MAX_BASELINE_BYTES) throw new Error("target session baseline exceeds the bounded content scan");
    }
    const afterRead = fstatSync(fd);
    if (afterRead.dev !== opened.dev || afterRead.ino !== opened.ino || afterRead.mode !== opened.mode ||
        afterRead.size !== opened.size || afterRead.mtimeMs !== opened.mtimeMs || afterRead.ctimeMs !== opened.ctimeMs) {
      throw new Error("target session file changed during read-only baseline scan");
    }
    hash.update("\0E\n");
  } finally { closeSync(fd); }
  samePathIdentity(path, stat);
}

function digestDirectory(path: string, relative: string, stat: Stats, hash: ReturnType<typeof createHash>, budget: ScanBudget, depth: number): void {
  if (!stat.isDirectory() || stat.isSymbolicLink() || depth > MAX_BASELINE_DEPTH) {
    throw new Error("target session contains an unsupported directory type or depth");
  }
  requirePrivateOwned(stat, "directory");
  budget.entries += 1;
  if (budget.entries > MAX_BASELINE_ENTRIES) throw new Error("target session baseline exceeds the bounded entry scan");
  hash.update(`D\0${relative}\0${stat.dev}\0${stat.ino}\0${stat.mode}\n`);
  const entries = readdirSync(path).sort();
  for (const name of entries) {
    if (!name || name === "." || name === ".." || name.includes("/") || name.includes("\\")) {
      throw new Error("target session contains an invalid entry name");
    }
    budget.entries += 1;
    if (budget.entries > MAX_BASELINE_ENTRIES) throw new Error("target session baseline exceeds the bounded entry scan");
    const childPath = join(path, name);
    const child = lstatSync(childPath);
    if (child.isSymbolicLink()) throw new Error("target session contains a symlink");
    const childRelative = relative ? `${relative}/${name}` : name;
    if (child.isDirectory()) digestDirectory(childPath, childRelative, child, hash, budget, depth + 1);
    else if (child.isFile()) digestRegularFile(childPath, childRelative, child, hash, budget);
    else throw new Error("target session contains an unsupported file type");
  }
  const afterNames = readdirSync(path).sort();
  if (afterNames.length !== entries.length || afterNames.some((name, index) => name !== entries[index])) {
    throw new Error("target session changed during read-only baseline scan");
  }
  samePathIdentity(path, stat);
  hash.update(`E\0${relative}\n`);
}

function snapshotOwnerlessSessionAt(
  userRoot: string,
  targetRoot: string,
  targetId: string,
  sessionId: string,
  processList: string,
  budget: ScanBudget,
): string {
  if (!SESSION_ID.test(sessionId)) throw new Error("target session entry blocks acceptance");
  const sessionRoot = join(targetRoot, sessionId);
  const rootStat = lstatSync(sessionRoot);
  if (rootStat.isSymbolicLink() || !rootStat.isDirectory()) throw new Error("target session entry blocks acceptance");
  requirePrivateOwned(rootStat, "root");
  requireAbsent(join(sessionRoot, "owner.json"), "target session owner evidence");
  requireAbsent(join(userRoot, "sessions", `.incodex-burned-${sessionId}.json`), "target session burn proof");
  requireNoProcessReference(sessionRoot, processList);
  const hash = createHash("sha256");
  hash.update(`session-baseline-v1\0${targetId}\0${sessionId}\0`);
  digestDirectory(sessionRoot, "", rootStat, hash, budget, 0);
  requireAbsent(join(sessionRoot, "owner.json"), "target session owner evidence");
  requireAbsent(join(userRoot, "sessions", `.incodex-burned-${sessionId}.json`), "target session burn proof");
  requireNoProcessReference(sessionRoot, processList);
  return hash.digest("hex");
}

function stableTargetEntries(targetRoot: string, expected: string[]): void {
  const before = lstatSync(targetRoot);
  if (before.isSymbolicLink() || !before.isDirectory()) throw new Error("target session root changed during read-only preflight");
  const initial = readdirSync(targetRoot).sort();
  if (initial.length !== expected.length || initial.some((name, index) => name !== expected[index])) {
    throw new Error("target session root changed during read-only preflight");
  }
  const after = lstatSync(targetRoot);
  const current = readdirSync(targetRoot).sort();
  if (after.isSymbolicLink() || !after.isDirectory() || before.dev !== after.dev || before.ino !== after.ino ||
      current.length !== expected.length || current.some((name, index) => name !== expected[index])) {
    throw new Error("target session root changed during read-only preflight");
  }
}

function captureOwnerlessSessionBaselineAtUnchecked(userRoot: string, targetId: string, processList?: string): SessionBaseline {
  const targetRoot = targetSessionRootAt(userRoot, targetId);
  if (!targetRoot) return new Map();
  const entries = readdirSync(targetRoot).sort();
  if (entries.length > MAX_BASELINE_SESSIONS) throw new Error("target session baseline exceeds the bounded session count");
  for (const name of entries) {
    const stat = lstatSync(join(targetRoot, name));
    if (!SESSION_ID.test(name) || stat.isSymbolicLink() || !stat.isDirectory()) {
      throw new Error("target session entry blocks acceptance");
    }
  }
  const processesBefore = processList ?? (entries.length ? readProcessList() : "");
  const baseline = new Map<string, string>();
  const budget: ScanBudget = { entries: 0, bytes: 0 };
  for (const sessionId of entries) {
    baseline.set(sessionId, snapshotOwnerlessSessionAt(userRoot, targetRoot, targetId, sessionId, processesBefore, budget));
  }
  const processesAfter = processList ?? (entries.length ? readProcessList() : "");
  for (const sessionId of entries) requireNoProcessReference(join(targetRoot, sessionId), processesAfter);
  stableTargetEntries(targetRoot, entries);
  if (targetSessionRootAt(userRoot, targetId) !== targetRoot) throw new Error("target session root changed during read-only preflight");
  return baseline;
}

export function captureOwnerlessSessionBaselineAt(userRoot: string, targetId: string, processList?: string): SessionBaseline {
  try { return captureOwnerlessSessionBaselineAtUnchecked(userRoot, targetId, processList); }
  catch (error) {
    const message = error instanceof Error ? error.message : "";
    const errorPath = isRecord(error) && typeof error.path === "string" ? error.path : "";
    if (message.includes(userRoot) || errorPath.includes(userRoot)) {
      throw new Error("target session evidence cannot be inspected safely");
    }
    throw error;
  }
}

export function requireSessionBaselineUnchangedAt(
  userRoot: string,
  targetId: string,
  baseline: SessionBaseline,
  processList?: string,
): SessionBaselineSummary {
  const targetRoot = targetSessionRootAt(userRoot, targetId);
  if (baseline.size > 0 && !targetRoot) throw new Error("target session baseline changed");
  if (!targetRoot) return summarizeSessionBaseline(baseline);
  const ids = [...baseline.keys()].sort();
  if (ids.some((sessionId) => !SESSION_ID.test(sessionId))) throw new Error("target session baseline identity is invalid");
  const processesBefore = processList ?? (ids.length ? readProcessList() : "");
  const budget: ScanBudget = { entries: 0, bytes: 0 };
  for (const sessionId of ids) {
    let digest: string;
    try { digest = snapshotOwnerlessSessionAt(userRoot, targetRoot, targetId, sessionId, processesBefore, budget); }
    catch { throw new Error("target session baseline changed"); }
    if (digest !== baseline.get(sessionId)) throw new Error("target session baseline changed");
  }
  const processesAfter = processList ?? (ids.length ? readProcessList() : "");
  for (const sessionId of ids) {
    try { requireNoProcessReference(join(targetRoot, sessionId), processesAfter); }
    catch { throw new Error("target session baseline changed"); }
  }
  if (targetSessionRootAt(userRoot, targetId) !== targetRoot) throw new Error("target session baseline changed");
  return summarizeSessionBaseline(baseline);
}

export function summarizeSessionBaseline(baseline: SessionBaseline): SessionBaselineSummary {
  const hash = createHash("sha256");
  const entries = [...baseline.entries()].sort(([left], [right]) => left.localeCompare(right));
  for (const [sessionId, digest] of entries) hash.update(`${sessionId}\0${digest}\n`);
  return { count: entries.length, digest: hash.digest("hex") };
}

export function addedSessionIds(before: ReadonlySet<string>, after: ReadonlySet<string>): string[] {
  return [...after].filter((sessionId) => !before.has(sessionId)).sort();
}
