import { createHash } from "node:crypto";
import { closeSync, existsSync, openSync, readSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { readPackageMain } from "./asar";
import { ASAR_REL, PLIST_REL } from "./paths";

export type AppListing = {
  bundleIdentifier: string;
  appVersion: string;
  appBuild: string;
  architecture: string;
};

export type AppIdentity = AppListing & {
  asarFileHash: string;
  plistFileHash: string;
};

export type AppInspection = {
  path: string;
  exists: boolean;
  asarExists: boolean;
  patched: boolean;
  installId: string | null;
  listing: AppListing | null;
  identity: AppIdentity | null;
  originalMain: string;
};

type PlistInfo = {
  bundleIdentifier: string;
  appVersion: string;
  appBuild: string;
  executable: string;
};

const PLIST_PY = `
import json, plistlib, sys
with open(sys.argv[1], "rb") as f:
    data = plistlib.load(f)
print(json.dumps({
    "bundleIdentifier": data.get("CFBundleIdentifier") or "",
    "appVersion": data.get("CFBundleShortVersionString") or "",
    "appBuild": str(data.get("CFBundleVersion") or ""),
    "executable": data.get("CFBundleExecutable") or "ChatGPT",
}))
`;

export function fileSha256(path: string): string {
  const hash = createHash("sha256");
  const fd = openSync(path, "r");
  try {
    const buf = Buffer.alloc(1024 * 1024);
    let bytes = 0;
    while ((bytes = readSync(fd, buf, 0, buf.length, null)) > 0) {
      hash.update(buf.subarray(0, bytes));
    }
    return hash.digest("hex");
  } finally {
    closeSync(fd);
  }
}

export function isCompleteListing(listing: AppListing): boolean {
  return Boolean(
    listing.bundleIdentifier && listing.appVersion && listing.appBuild && listing.architecture,
  );
}

export function isCompleteIdentity(identity: AppIdentity): boolean {
  return isCompleteListing(identity) && Boolean(identity.asarFileHash && identity.plistFileHash);
}

export function listingsEqual(left: AppListing, right: AppListing): boolean {
  return (
    left.bundleIdentifier === right.bundleIdentifier &&
    left.appVersion === right.appVersion &&
    left.appBuild === right.appBuild &&
    left.architecture === right.architecture
  );
}

export function identitiesEqual(left: AppIdentity, right: AppIdentity): boolean {
  return (
    listingsEqual(left, right) &&
    left.asarFileHash === right.asarFileHash &&
    left.plistFileHash === right.plistFileHash
  );
}

export function inspectApp(appPath: string): AppInspection {
  const missing: AppInspection = {
    path: appPath,
    exists: false,
    asarExists: false,
    patched: false,
    installId: null,
    listing: null,
    identity: null,
    originalMain: "",
  };
  if (!existsSync(appPath)) return missing;

  const asarPath = join(appPath, ASAR_REL);
  const plistPath = join(appPath, PLIST_REL);
  const asarExists = existsSync(asarPath);
  if (!asarExists || !existsSync(plistPath)) {
    return { ...missing, exists: true, asarExists };
  }

  const pkg = readPackageMain(asarPath);
  const info = readPlistInfo(plistPath);
  const executable = join(appPath, "Contents/MacOS", info.executable || "ChatGPT");
  const listing: AppListing = {
    bundleIdentifier: info.bundleIdentifier,
    appVersion: info.appVersion,
    appBuild: info.appBuild,
    architecture: existsSync(executable) ? readArchitecture(executable) : "",
  };
  const identity: AppIdentity = {
    ...listing,
    asarFileHash: fileSha256(asarPath),
    plistFileHash: fileSha256(plistPath),
  };
  return {
    path: appPath,
    exists: true,
    asarExists: true,
    patched: pkg.alreadyPatched,
    installId: pkg.installId,
    listing: isCompleteListing(listing) ? listing : null,
    identity: isCompleteIdentity(identity) ? identity : null,
    originalMain: pkg.main,
  };
}

function readPlistInfo(plistPath: string): PlistInfo {
  const result = spawnSync("python3", ["-c", PLIST_PY, plistPath], { encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(result.stderr || `failed to read Info.plist: ${plistPath}`);
  }
  return JSON.parse(result.stdout) as PlistInfo;
}

function readArchitecture(executablePath: string): string {
  const lipo = spawnSync("lipo", ["-archs", executablePath], { encoding: "utf8" });
  if (lipo.status === 0 && lipo.stdout.trim()) {
    return lipo.stdout
      .trim()
      .split(/\s+/)
      .filter(Boolean)
      .sort()
      .join(" ");
  }
  return "";
}
