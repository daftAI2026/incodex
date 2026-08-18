import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export type SignObservation = {
  path: string;
  identifier: string;
  hardenedRuntime: boolean;
  entitlementsKept: boolean;
  skippedEntitlements?: string;
};

export function orderForInsideOut(paths: string[], appPath: string): string[] {
  const unique = [...new Set(paths.filter((item) => item && item !== appPath))];
  unique.sort((left, right) => {
    const depth = right.split("/").length - left.split("/").length;
    if (depth !== 0) return depth;
    return right.length - left.length;
  });
  return [...unique, appPath];
}

export function discoverNestedCode(appPath: string): string[] {
  const listed = spawnSync(
    "find",
    [
      appPath,
      "(",
      "-name",
      "*.framework",
      "-o",
      "-name",
      "*.appex",
      "-o",
      "-name",
      "*.xpc",
      "-o",
      "-name",
      "*.dylib",
      "-o",
      "-name",
      "*.bundle",
      ")",
    ],
    { encoding: "utf8" },
  );
  const found = (listed.stdout || "")
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  const macos = spawnSync("find", [join(appPath, "Contents/MacOS"), "-type", "f"], { encoding: "utf8" });
  const binaries = (macos.stdout || "")
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  return [...found, ...binaries];
}

export function displayField(target: string, flag: string): string {
  const result = spawnSync("codesign", ["--display", flag, target], { encoding: "utf8" });
  return `${result.stdout || ""}${result.stderr || ""}`;
}

export function dumpEntitlements(target: string): string | null {
  const result = spawnSync("codesign", ["--display", "--entitlements", ":-", target], { encoding: "utf8" });
  const xml = result.stdout || "";
  if (!xml.includes("<plist")) return null;
  return xml;
}

export function hasHardenedRuntime(target: string): boolean {
  const text = displayField(target, "--verbose=2");
  return /flags=.*runtime/.test(text) || /\(runtime\)/.test(text);
}

export function signOne(target: string, entitlements: string | null, hardened: boolean): void {
  const args = ["--force", "--sign", "-"];
  if (hardened) args.push("--options", "runtime");
  let entitlementsFile: string | null = null;
  if (entitlements) {
    entitlementsFile = join(mkdtempSync(join(tmpdir(), "incodex-ent-")), "entitlements.plist");
    writeFileSync(entitlementsFile, entitlements);
    args.push("--entitlements", entitlementsFile);
  }
  args.push(target);
  const result = spawnSync("codesign", args, { encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(result.stderr || `codesign failed: ${target}`);
  }
}

export function signApp(appPath: string): SignObservation[] {
  const nested = discoverNestedCode(appPath);
  const order = orderForInsideOut(nested, appPath);
  const observations: SignObservation[] = [];
  for (const target of order) {
    const entitlements = dumpEntitlements(target);
    const hardened = hasHardenedRuntime(target);
    signOne(target, entitlements, hardened);
    observations.push({
      path: target,
      identifier: displayField(target, "--verbose").match(/Identifier=(\S+)/)?.[1] || "",
      hardenedRuntime: hardened,
      entitlementsKept: Boolean(entitlements),
      skippedEntitlements: entitlements ? undefined : "adhoc cannot preserve original sealed identity",
    });
  }
  return observations;
}

export function verifyApp(appPath: string): boolean {
  const result = spawnSync(
    "codesign",
    ["--verify", "--deep", "--strict", "--verbose=4", appPath],
    { encoding: "utf8" },
  );
  return result.status === 0;
}
