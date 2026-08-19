import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { closeSync, existsSync, mkdirSync, openSync, readFileSync, unlinkSync, writeSync } from "node:fs";
import { join } from "node:path";
import { canonicalPath } from "./canonical-target";
import { USER_ROOT } from "./paths";

export type TargetLock = {
  path: string;
  realPath: string;
};

export type TargetLockRequest = {
  targetPath: string;
  command: string;
  installId?: string;
  root?: string;
};

type LockRecord = {
  schemaVersion: 1;
  pid: number;
  processStart: string;
  command: string;
  installId?: string;
  requestedPath: string;
  realPath: string;
  createdAt: string;
};

export function lockPathFor(targetPath: string, root = USER_ROOT): string {
  const digest = createHash("sha256").update(canonicalPath(targetPath)).digest("hex");
  return join(root, "locks", `${digest}.lock`);
}

export function acquireTargetLock(request: TargetLockRequest): TargetLock {
  const root = request.root ?? USER_ROOT;
  const realPath = canonicalPath(request.targetPath);
  const path = lockPathFor(request.targetPath, root);
  mkdirSync(join(root, "locks"), { recursive: true, mode: 0o700 });
  const record: LockRecord = {
    schemaVersion: 1,
    pid: process.pid,
    processStart: processStart(process.pid) ?? "",
    command: request.command,
    installId: request.installId,
    requestedPath: request.targetPath,
    realPath,
    createdAt: new Date().toISOString(),
  };
  try {
    writeExclusive(path, record);
    return { path, realPath };
  } catch (error) {
    if (!isExistError(error)) throw error;
    if (stealIfStale(path)) {
      writeExclusive(path, record);
      return { path, realPath };
    }
    const holder = readLock(path);
    const who = holder ? `${holder.command} pid ${holder.pid}` : "another process";
    throw new Error(`another incodex command is modifying this app (${who})`);
  }
}

export function releaseTargetLock(lock: TargetLock): void {
  try {
    const holder = readLock(lock.path);
    if (holder && holder.pid !== process.pid) return;
    unlinkSync(lock.path);
  } catch (error) {
    if (isNotFound(error)) return;
    throw error;
  }
}

export function withTargetLock<T>(request: TargetLockRequest, fn: () => T): T {
  const lock = acquireTargetLock(request);
  try {
    return fn();
  } finally {
    releaseTargetLock(lock);
  }
}

export async function withTargetLockAsync<T>(request: TargetLockRequest, fn: () => Promise<T>): Promise<T> {
  const lock = acquireTargetLock(request);
  try {
    return await fn();
  } finally {
    releaseTargetLock(lock);
  }
}

function writeExclusive(path: string, record: LockRecord): void {
  const fd = openSync(path, "wx", 0o600);
  try {
    writeSync(fd, `${JSON.stringify(record, null, 2)}\n`);
  } finally {
    closeSync(fd);
  }
}

function stealIfStale(path: string): boolean {
  const holder = readLock(path);
  if (!holder) {
    try {
      unlinkSync(path);
      return true;
    } catch {
      return false;
    }
  }
  if (lockIsLive(holder)) return false;
  try {
    unlinkSync(path);
    return true;
  } catch {
    return false;
  }
}

function lockIsLive(holder: LockRecord): boolean {
  if (!pidAlive(holder.pid)) return false;
  if (!holder.processStart) return true;
  const current = processStart(holder.pid);
  if (!current) return true;
  return current === holder.processStart;
}

function readLock(path: string): LockRecord | null {
  if (!existsSync(path)) return null;
  try {
    const raw: unknown = JSON.parse(readFileSync(path, "utf8"));
    if (!raw || typeof raw !== "object") return null;
    const value = raw as Partial<LockRecord>;
    if (value.schemaVersion !== 1) return null;
    if (typeof value.pid !== "number" || typeof value.command !== "string") return null;
    if (typeof value.realPath !== "string") return null;
    return value as LockRecord;
  } catch {
    return null;
  }
}

function pidAlive(pid: number): boolean {
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function processStart(pid: number): string | null {
  const listed = spawnSync("ps", ["-p", String(pid), "-o", "lstart="], { encoding: "utf8" });
  if (listed.status !== 0) return null;
  const start = (listed.stdout || "").trim();
  return start || null;
}

function isExistError(error: unknown): boolean {
  return Boolean(error && typeof error === "object" && "code" in error && error.code === "EEXIST");
}

function isNotFound(error: unknown): boolean {
  return Boolean(error && typeof error === "object" && "code" in error && error.code === "ENOENT");
}
