import { existsSync, mkdirSync, renameSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";

export type SwapOps = {
  rename: (from: string, to: string) => void;
  remove: (path: string) => void;
};

export type SwapOptions = {
  outgoing?: string;
  afterTargetMoved?: () => void;
};

export const defaultSwapOps: SwapOps = {
  rename: renameSync,
  remove: (path) => rmSync(path, { recursive: true, force: true }),
};

export function outgoingPath(targetApp: string): string {
  return `${targetApp}.incodex-outgoing`;
}

export function transactionOutgoing(root: string, installId: string): string {
  return join(root, "transactions", installId, "outgoing", "ChatGPT.app");
}

export function swapBundle(
  stagedApp: string,
  targetApp: string,
  ops: SwapOps = defaultSwapOps,
  options: SwapOptions = {},
): void {
  const outgoing = options.outgoing ?? outgoingPath(targetApp);
  if (existsSync(outgoing)) {
    throw new Error(`outgoing already exists: ${outgoing}`);
  }
  mkdirSync(dirname(outgoing), { recursive: true });
  ops.rename(targetApp, outgoing);
  options.afterTargetMoved?.();
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
}

export function restoreOutgoingIfNeeded(
  targetApp: string,
  outgoing = outgoingPath(targetApp),
  ops: SwapOps = defaultSwapOps,
): boolean {
  if (!existsSync(outgoing)) return false;
  if (!existsSync(targetApp)) {
    ops.rename(outgoing, targetApp);
    return true;
  }
  return false;
}
