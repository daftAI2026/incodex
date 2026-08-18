import { existsSync, renameSync, rmSync } from "node:fs";

export type SwapOps = {
  rename: (from: string, to: string) => void;
  remove: (path: string) => void;
};

export const defaultSwapOps: SwapOps = {
  rename: renameSync,
  remove: (path) => rmSync(path, { recursive: true, force: true }),
};

export function outgoingPath(targetApp: string): string {
  return `${targetApp}.incodex-outgoing`;
}

export function swapBundle(stagedApp: string, targetApp: string, ops: SwapOps = defaultSwapOps): void {
  const outgoing = outgoingPath(targetApp);
  ops.remove(outgoing);
  ops.rename(targetApp, outgoing);
  try {
    ops.rename(stagedApp, targetApp);
  } catch (error) {
    try {
      ops.rename(outgoing, targetApp);
    } catch (rollbackError) {
      const rollback = rollbackError instanceof Error ? rollbackError.message : String(rollbackError);
      const first = error instanceof Error ? error.message : String(error);
      throw new Error(`swap failed (${first}) and rollback rename failed (${rollback})`);
    }
    throw error;
  }
  ops.remove(outgoing);
}

export function restoreOutgoingIfNeeded(targetApp: string, ops: SwapOps = defaultSwapOps): boolean {
  const outgoing = outgoingPath(targetApp);
  if (!existsSync(outgoing)) return false;
  if (!existsSync(targetApp)) {
    ops.rename(outgoing, targetApp);
    return true;
  }
  return false;
}
