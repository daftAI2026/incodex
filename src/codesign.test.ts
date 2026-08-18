import { describe, expect, test } from "bun:test";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  CODESIGN_VERIFY_ARGS,
  classifyUnretainable,
  compareSigning,
  entitlementKeys,
  isSubsetOfHostEntitlements,
  normalizeEntitlementKeys,
  stripUnretainableEntitlements,
  withDisableLibraryValidation,
  diagnoseSpctl,
  orderForInsideOut,
  signApp,
  signOne,
  signingManifestFrom,
  verifyApp,
  type SigningComponent,
} from "./codesign";

describe("inside-out signing order", () => {
  test("signs nested frameworks before the top-level app and never uses --deep for order", () => {
    const app = "/tmp/ChatGPT.app";
    const order = orderForInsideOut(
      [
        app,
        `${app}/Contents/Frameworks/Electron Framework.framework`,
        `${app}/Contents/Frameworks/Electron Framework.framework/Versions/A/Libraries/libffmpeg.dylib`,
        `${app}/Contents/Frameworks/Squirrel.framework`,
        `${app}/Contents/MacOS/ChatGPT`,
      ],
      app,
    );
    expect(order.at(-1)).toBe(app);
    expect(order.indexOf(`${app}/Contents/Frameworks/Electron Framework.framework/Versions/A/Libraries/libffmpeg.dylib`)).toBeLessThan(
      order.indexOf(`${app}/Contents/Frameworks/Electron Framework.framework`),
    );
  });
});

describe("adhoc entitlement policy", () => {
  test("records team-bound entitlements that ad hoc identity cannot legally keep", () => {
    const xml = `<?xml version="1.0"?><plist><dict>
      <key>com.apple.security.cs.allow-jit</key><true/>
      <key>com.apple.developer.team-identifier</key><string>ABCD123456</string>
      <key>keychain-access-groups</key><array><string>ABCD123456.com.test</string></array>
    </dict></plist>`;
    expect(classifyUnretainable(xml)).toEqual([
      "com.apple.developer.team-identifier",
      "keychain-access-groups",
    ]);
    expect(classifyUnretainable(null)).toEqual([]);
  });

  test("strips team-bound keys before adhoc sign so launchd is not given a fake Team ID", () => {
    const xml = `<?xml version="1.0"?><plist><dict>
      <key>com.apple.application-identifier</key><string>ABCD.com.test</string>
      <key>com.apple.developer.team-identifier</key><string>ABCD</string>
      <key>com.apple.security.cs.allow-jit</key><true/>
      <key>keychain-access-groups</key><array><string>ABCD.com.test</string></array>
    </dict></plist>`;
    const stripped = stripUnretainableEntitlements(xml);
    expect(entitlementKeys(stripped)).toEqual(["com.apple.security.cs.allow-jit"]);
    expect(withDisableLibraryValidation(stripped)).toContain("disable-library-validation");
  });

  test("compareSigning fails if hardened runtime or entitlement keys disappear", () => {
    const before: SigningComponent[] = [
      {
        path: "/tmp/Mini.app",
        relativePath: ".",
        identifier: "com.incodex.mini",
        hardenedRuntime: true,
        requirements: "designated => identifier com.incodex.mini",
        entitlementsXml: "<plist><dict><key>com.apple.security.cs.allow-jit</key><true/></dict></plist>",
        entitlementsHash: "a",
        entitlementsOwned: true,
      },
    ];
    const afterOk: SigningComponent[] = [
      {
        ...before[0]!,
        requirements: "designated => cdhash H\"abc\"",
      },
    ];
    const ok = compareSigning(before, afterOk);
    expect(ok.reasons).toEqual([]);
    expect(ok.observations[0]?.requirementsChanged).toBe(true);

    const afterDropped = compareSigning(before, [{ ...before[0]!, hardenedRuntime: false, entitlementsXml: null, entitlementsHash: null, entitlementsOwned: false }]);
    expect(afterDropped.reasons.some((reason) => reason.includes("hardened runtime"))).toBe(true);
    expect(afterDropped.reasons.some((reason) => reason.includes("entitlements dropped"))).toBe(true);
  });

  test("does not treat host-app entitlements echoed onto a nested dylib as owned", () => {
    const hostXml = "<plist><dict><key>com.apple.security.cs.allow-jit</key><true/></dict></plist>";
    const dylib: SigningComponent = {
      path: "/tmp/Mini.app/Contents/Frameworks/libfoo.dylib",
      relativePath: "Contents/Frameworks/libfoo.dylib",
      identifier: "libfoo",
      hardenedRuntime: true,
      requirements: "designated => identifier libfoo",
      entitlementsXml: null,
      entitlementsHash: null,
      entitlementsOwned: false,
    };
    const after = compareSigning(
      [dylib],
      [{ ...dylib, entitlementsXml: null, requirements: "designated => cdhash H\"abc\"" }],
    );
    expect(after.reasons).toEqual([]);
    expect(normalizeEntitlementKeys(hostXml)).toContain("com.apple.security.cs.allow-jit");
    const host = `<plist><dict>
      <key>com.apple.developer.team-identifier</key><string>ABCD</string>
      <key>com.apple.security.cs.allow-jit</key><true/>
      <key>com.apple.security.network.client</key><true/>
    </dict></plist>`;
    const echoed = `<plist><dict>
      <key>com.apple.security.cs.allow-jit</key><true/>
      <key>com.apple.security.network.client</key><true/>
    </dict></plist>`;
    expect(isSubsetOfHostEntitlements(echoed, host)).toBe(true);
    expect(isSubsetOfHostEntitlements(host, echoed)).toBe(false);
  });

  test("signing manifest records unretainable keys from the pre-resign dump", () => {
    const xml = `<plist><dict>
      <key>com.apple.security.cs.allow-jit</key><true/>
      <key>com.apple.developer.team-identifier</key><string>ABCD123456</string>
    </dict></plist>`;
    const component: SigningComponent = {
      path: "/tmp/Mini.app",
      relativePath: ".",
      identifier: "com.incodex.mini",
      hardenedRuntime: true,
      requirements: "designated => identifier com.incodex.mini",
      entitlementsXml: xml,
      entitlementsHash: "a",
      entitlementsOwned: true,
    };
    const manifest = signingManifestFrom({
      appPath: "/tmp/Mini.app",
      before: [component],
      after: [{ ...component, requirements: "designated => cdhash H\"abc\"" }],
      verified: true,
      spctl: { status: 3, output: "rejected", accepted: false, usedAsSuccessGate: false },
    });
    expect(manifest.unretainableEntitlements).toEqual([
      {
        relativePath: ".",
        keys: ["com.apple.developer.team-identifier"],
        reason: "adhoc identity cannot legally retain team-bound entitlements",
      },
    ]);
    expect(manifest.spctl.usedAsSuccessGate).toBe(false);
  });
});

describe("verify vs spctl", () => {
  test("verifyApp only uses codesign --verify and never consults Gatekeeper", () => {
    const src = readFileSync(join(import.meta.dir, "codesign.ts"), "utf8");
    const start = src.indexOf("export function verifyApp");
    const end = src.indexOf("export function verifyTarget");
    const body = src.slice(start, end);
    expect(body).toContain("CODESIGN_VERIFY_ARGS");
    expect(body).not.toContain("spctl");
    expect(CODESIGN_VERIFY_ARGS).toEqual(["--verify", "--deep", "--strict", "--verbose=4"]);
  });

  test("spctl diagnosis is recorded and is never a success gate", () => {
    const diagnosis = diagnoseSpctl("/tmp/incodex-missing-for-spctl.app");
    expect(diagnosis.usedAsSuccessGate).toBe(false);
    expect(diagnosis.accepted).toBe(false);
    expect(diagnosis.status).not.toBe(0);
  });
});

describe("inside-out resign of a mini app", () => {
  test("keeps helper entitlements and hardened runtime, records unretainable keys, and verifies the target", () => {
    const root = mkdtempSync(join(tmpdir(), "incodex-sign-"));
    const app = join(root, "Mini.app");
    mkdirSync(join(app, "Contents/MacOS"), { recursive: true });
    writeFileSync(
      join(app, "Contents/Info.plist"),
      `<?xml version="1.0"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.incodex.mini</string>
  <key>CFBundleName</key><string>Mini</string>
  <key>CFBundleExecutable</key><string>Mini</string>
  <key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
`,
    );
    const main = join(app, "Contents/MacOS/Mini");
    const helper = join(app, "Contents/MacOS/Helper");
    writeFileSync(main, "#!/bin/sh\necho mini\n");
    writeFileSync(helper, "#!/bin/sh\necho helper\n");
    chmodSync(main, 0o755);
    chmodSync(helper, 0o755);

    const entitlements = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>com.apple.security.cs.allow-jit</key><true/>
  <key>com.apple.developer.team-identifier</key><string>ABCD123456</string>
  <key>keychain-access-groups</key><array><string>ABCD123456.com.incodex.mini</string></array>
</dict></plist>`;
    signOne(helper, entitlements, true);
    signOne(main, entitlements, true);
    signOne(app, entitlements, true);

    const manifest = signApp(app);
    expect(manifest.verified).toBe(true);
    expect(verifyApp(app)).toBe(true);
    expect(manifest.spctl.usedAsSuccessGate).toBe(false);
    expect(verifyApp(helper)).toBe(true);
  });
});
