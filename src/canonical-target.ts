import { realpathSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { DEFAULT_APP } from "./paths";

export type CanonicalTarget = {
  requestedPath: string;
  realPath: string;
  isOfficial: boolean;
};

export function canonicalPath(inputPath: string): string {
  const requested = resolve(inputPath);
  try {
    return realpathSync.native(requested);
  } catch {
    const parent = dirname(requested);
    try {
      return join(realpathSync.native(parent), basename(requested));
    } catch {
      return requested;
    }
  }
}

export function canonicalize(inputPath: string, officialPath = DEFAULT_APP): CanonicalTarget {
  const requestedPath = resolve(inputPath);
  const realPath = canonicalPath(inputPath);
  return {
    requestedPath,
    realPath,
    isOfficial: realPath === canonicalPath(officialPath),
  };
}

export function isOfficialApp(appPath: string, officialPath = DEFAULT_APP): boolean {
  return canonicalize(appPath, officialPath).isOfficial;
}
