import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";

export const CODESIGN_VERIFY_ARGS = ["--verify", "--deep", "--strict", "--verbose=4"] as const;

const ADHOC_UNRETAINABLE_KEYS = [
  "com.apple.developer.team-identifier",
  "com.apple.application-identifier",
  "com.apple.security.application-groups",
  "keychain-access-groups",
] as const;

export type SignObservation = {
  path: string;
  identifier: string;
  hardenedRuntime: boolean;
  entitlementsKept: boolean;
  skippedEntitlements?: string;
  requirementsChanged?: boolean;
};

export type SigningComponent = {
  path: string;
  relativePath: string;
  identifier: string;
  hardenedRuntime: boolean;
  requirements: string;
  entitlementsXml: string | null;
  entitlementsHash: string | null;
  entitlementsOwned: boolean;
};

export type UnretainableEntitlement = {
  relativePath: string;
  keys: string[];
  reason: string;
};

export type SpctlDiagnosis = {
  status: number;
  output: string;
  accepted: boolean;
  usedAsSuccessGate: false;
};

export type SigningManifest = {
  schemaVersion: 1;
  appPath: string;
  verified: boolean;
  spctl: SpctlDiagnosis;
  components: SigningComponent[];
  observations: SignObservation[];
  unretainableEntitlements: UnretainableEntitlement[];
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

export function dumpRequirements(target: string): string {
  const result = spawnSync("codesign", ["--display", "--requirements", "-", target], { encoding: "utf8" });
  return `${result.stdout || ""}${result.stderr || ""}`.trim();
}

export function hasHardenedRuntime(target: string): boolean {
  const text = displayField(target, "--verbose=2");
  return /flags=.*runtime/.test(text) || /\(runtime\)/.test(text);
}

export function entitlementKeys(xml: string | null): string[] {
  if (!xml) return [];
  return [...xml.matchAll(/<key>([^<]+)<\/key>/g)].map((match) => match[1] ?? "").filter(Boolean);
}

export function classifyUnretainable(xml: string | null): string[] {
  return entitlementKeys(xml).filter((key) =>
    (ADHOC_UNRETAINABLE_KEYS as readonly string[]).includes(key),
  );
}

export function normalizeEntitlementKeys(xml: string | null): string {
  return entitlementKeys(xml).slice().sort().join("\n");
}

/** codesign --display on nested dylibs often echoes the host app plist. Those are not the library's own entitlements. */
export function ownEntitlementsXml(target: string, appPath: string, hostXml: string | null): string | null {
  const xml = dumpEntitlements(target);
  if (!xml) return null;
  if (target === appPath) return xml;
  if (hostXml && normalizeEntitlementKeys(xml) === normalizeEntitlementKeys(hostXml)) return null;
  return xml;
}

export function inspectComponent(appPath: string, target: string, hostXml: string | null = null): SigningComponent {
  const entitlementsXml = ownEntitlementsXml(target, appPath, hostXml ?? dumpEntitlements(appPath));
  return {
    path: target,
    relativePath: relative(appPath, target) || ".",
    identifier: displayField(target, "--verbose").match(/Identifier=(\S+)/)?.[1] || "",
    hardenedRuntime: hasHardenedRuntime(target),
    requirements: dumpRequirements(target),
    entitlementsXml,
    entitlementsHash: entitlementsXml
      ? createHash("sha256").update(entitlementsXml).digest("hex")
      : null,
    entitlementsOwned: Boolean(entitlementsXml),
  };
}

export function inspectSigning(appPath: string): SigningComponent[] {
  const hostXml = dumpEntitlements(appPath);
  const nested = discoverNestedCode(appPath);
  const order = orderForInsideOut(nested, appPath);
  return order.map((target) => inspectComponent(appPath, target, hostXml));
}

export function diagnoseSpctl(appPath: string): SpctlDiagnosis {
  const result = spawnSync("spctl", ["--assess", "--verbose=4", appPath], { encoding: "utf8" });
  const output = `${result.stdout || ""}${result.stderr || ""}`.trim();
  return {
    status: result.status ?? 1,
    output,
    accepted: result.status === 0 && /accepted/i.test(output),
    usedAsSuccessGate: false,
  };
}

export function compareSigning(
  before: SigningComponent[],
  after: SigningComponent[],
): { observations: SignObservation[]; reasons: string[] } {
  const afterByPath = new Map(after.map((item) => [item.relativePath, item]));
  const observations: SignObservation[] = [];
  const reasons: string[] = [];
  for (const original of before) {
    const next = afterByPath.get(original.relativePath);
    if (!next) {
      reasons.push(`missing nested code after resign: ${original.relativePath}`);
      continue;
    }
    const entitlementKeysLost = original.entitlementsOwned
      ? entitlementKeys(original.entitlementsXml).filter(
          (key) => !entitlementKeys(next.entitlementsXml).includes(key),
        )
      : [];
    const requirementsChanged = normalizeReq(original.requirements) !== normalizeReq(next.requirements);
    if (original.hardenedRuntime && !next.hardenedRuntime) {
      reasons.push(`hardened runtime dropped: ${original.relativePath}`);
    }
    if (entitlementKeysLost.length > 0) {
      reasons.push(`entitlements dropped on ${original.relativePath}: ${entitlementKeysLost.join(", ")}`);
    }
    observations.push({
      path: next.path,
      identifier: next.identifier,
      hardenedRuntime: next.hardenedRuntime,
      entitlementsKept: entitlementKeysLost.length === 0,
      skippedEntitlements:
        classifyUnretainable(original.entitlementsXml).length > 0
          ? "adhoc cannot honor team-bound entitlements"
          : original.entitlementsXml
            ? undefined
            : "adhoc cannot preserve original sealed identity",
      requirementsChanged,
    });
  }
  return { observations, reasons };
}

export function signOne(target: string, entitlements: string | null, hardened: boolean): void {
  const args = ["--force", "--sign", "-"];
  if (hardened) args.push("--options", "runtime");
  if (entitlements) {
    const entitlementsFile = join(mkdtempSync(join(tmpdir(), "incodex-ent-")), "entitlements.plist");
    writeFileSync(entitlementsFile, entitlements);
    args.push("--entitlements", entitlementsFile);
  }
  args.push(target);
  const result = spawnSync("codesign", args, { encoding: "utf8" });
  if (result.status !== 0) {
    throw new Error(result.stderr || `codesign failed: ${target}`);
  }
}

export function signApp(appPath: string): SigningManifest {
  const before = inspectSigning(appPath);
  const nested = discoverNestedCode(appPath);
  const order = orderForInsideOut(nested, appPath);
  for (const target of order) {
    const original = before.find((item) => item.path === target);
    signOne(
      target,
      original?.entitlementsOwned ? original.entitlementsXml : null,
      original?.hardenedRuntime ?? hasHardenedRuntime(target),
    );
  }
  const after = inspectSigning(appPath);
  const verified = verifyApp(appPath);
  if (!verified) {
    throw new Error("codesign --verify failed after inside-out resign");
  }
  return signingManifestFrom({
    appPath,
    before,
    after,
    verified,
    spctl: diagnoseSpctl(appPath),
  });
}

export function signingManifestFrom(input: {
  appPath: string;
  before: SigningComponent[];
  after: SigningComponent[];
  verified: boolean;
  spctl: SpctlDiagnosis;
}): SigningManifest {
  const compared = compareSigning(input.before, input.after);
  if (compared.reasons.length > 0) {
    throw new Error(`signing policy failed: ${compared.reasons.join("; ")}`);
  }
  return {
    schemaVersion: 1,
    appPath: input.appPath,
    verified: input.verified,
    spctl: input.spctl,
    components: input.after,
    observations: compared.observations,
    unretainableEntitlements: input.before
      .map((item) => ({
        relativePath: item.relativePath,
        keys: classifyUnretainable(item.entitlementsXml),
        reason: "adhoc identity cannot legally retain team-bound entitlements",
      }))
      .filter((item) => item.keys.length > 0),
  };
}

export function verifyApp(appPath: string): boolean {
  const result = spawnSync("codesign", [...CODESIGN_VERIFY_ARGS, appPath], { encoding: "utf8" });
  return result.status === 0;
}

export function verifyTarget(appPath: string, label: string): void {
  if (!verifyApp(appPath)) {
    throw new Error(`${label}: codesign --verify failed; refusing to treat the target as installed`);
  }
}

function normalizeReq(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}
