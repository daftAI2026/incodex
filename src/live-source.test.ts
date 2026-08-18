import { describe, expect, test } from "bun:test";
import type { AppIdentity, AppInspection } from "./app-identity";
import {
  NOT_INCODEX_INSTALL,
  canRestoreOfficial,
  liveRecordFromManifest,
  officialBackupMatchesInstall,
  parseLiveInstallRecord,
  selectOfficialInstallSource,
  type LiveInstallRecord,
} from "./live-source";

const originalA: AppIdentity = {
  bundleIdentifier: "com.openai.codex",
  appVersion: "1.0.0",
  appBuild: "100",
  architecture: "arm64",
  asarFileHash: "asar-a",
  plistFileHash: "plist-a",
};

const originalB: AppIdentity = {
  ...originalA,
  appVersion: "2.0.0",
  appBuild: "200",
  asarFileHash: "asar-b",
  plistFileHash: "plist-b",
};

const installId = "install-a";

function inspect(partial: Partial<AppInspection> & Pick<AppInspection, "path">): AppInspection {
  return {
    exists: true,
    asarExists: true,
    patched: false,
    installId: null,
    listing: null,
    identity: null,
    originalMain: "index.js",
    ...partial,
  };
}

function recordFor(original: AppIdentity, id = installId): LiveInstallRecord {
  return {
    schemaVersion: 1,
    installId: id,
    targetRealPath: "/Applications/ChatGPT.app",
    original,
    createdAt: "2026-08-18T00:00:00.000Z",
  };
}

describe("parseLiveInstallRecord", () => {
  test("rejects a record that is only 'looks unpatched' without identity hashes", () => {
    expect(parseLiveInstallRecord({ schemaVersion: 1, installId: "x" })).toBeNull();
    expect(parseLiveInstallRecord(recordFor(originalA))).toEqual(recordFor(originalA));
  });

  test("installation manifests carry full file hashes, not only ASAR header hashes", () => {
    const record = liveRecordFromManifest({
      schemaVersion: 1,
      installId,
      targetRealPath: "/Applications/ChatGPT.app",
      bundleIdentifier: originalA.bundleIdentifier,
      appVersion: originalA.appVersion,
      appBuild: originalA.appBuild,
      architecture: originalA.architecture,
      originalAsarHeaderHash: "header-only-is-not-enough",
      originalAsarFileHash: originalA.asarFileHash,
      originalPlistFileHash: originalA.plistFileHash,
      patchedAsarHeaderHash: "header-patched",
      patchedAsarFileHash: "file-patched",
      originalMain: "index.js",
      runtimeVersion: "0.1.0",
      createdAt: "2026-08-18T00:00:00.000Z",
      transactionState: "committed",
    });
    expect(record.original.asarFileHash).toBe("asar-a");
    expect(record.original.asarFileHash).not.toBe("header-only-is-not-enough");
  });
});

describe("selectOfficialInstallSource", () => {
  test("unpatched current app is the only source, even if an old unpatched backup exists", () => {
    const decision = selectOfficialInstallSource({
      current: inspect({
        path: "/Applications/ChatGPT.app",
        patched: false,
        listing: originalB,
        identity: originalB,
      }),
      backup: inspect({
        path: "ChatGPT.app.pre-live",
        patched: false,
        listing: originalA,
        identity: originalA,
      }),
      record: recordFor(originalA),
    });
    expect(decision).toEqual({ action: "use-current" });
  });

  test("first install with no backup uses the current official app", () => {
    const decision = selectOfficialInstallSource({
      current: inspect({
        path: "/Applications/ChatGPT.app",
        patched: false,
        listing: originalA,
        identity: originalA,
      }),
      backup: null,
      record: null,
    });
    expect(decision).toEqual({ action: "use-current" });
  });

  test("does not choose a backup just because it looks unpatched", () => {
    const decision = selectOfficialInstallSource({
      current: inspect({
        path: "/Applications/ChatGPT.app",
        patched: true,
        installId,
        listing: originalB,
      }),
      backup: inspect({
        path: "ChatGPT.app.pre-live",
        patched: false,
        identity: originalA,
      }),
      record: null,
    });
    expect(decision.action).toBe("reject");
  });

  test("patched current app may reuse the original backup only when the install record matches", () => {
    const decision = selectOfficialInstallSource({
      current: inspect({
        path: "/Applications/ChatGPT.app",
        patched: true,
        installId,
        listing: originalA,
      }),
      backup: inspect({
        path: "ChatGPT.app.pre-live",
        patched: false,
        identity: originalA,
      }),
      record: recordFor(originalA),
    });
    expect(decision).toEqual({ action: "use-backup" });
  });

  test("rejects a version-mismatched backup even when the current app is still patched", () => {
    const decision = selectOfficialInstallSource({
      current: inspect({
        path: "/Applications/ChatGPT.app",
        patched: true,
        installId,
        listing: originalB,
      }),
      backup: inspect({
        path: "ChatGPT.app.pre-live",
        patched: false,
        identity: originalA,
      }),
      record: recordFor(originalA),
    });
    expect(decision.action).toBe("reject");
  });

  test("rejects when install IDs do not match", () => {
    const decision = selectOfficialInstallSource({
      current: inspect({
        path: "/Applications/ChatGPT.app",
        patched: true,
        installId: "other-id",
        listing: originalA,
      }),
      backup: inspect({
        path: "ChatGPT.app.pre-live",
        patched: false,
        identity: originalA,
      }),
      record: recordFor(originalA),
    });
    expect(decision.action).toBe("reject");
  });

  test("rejects when backup hashes do not match the recorded original", () => {
    const decision = selectOfficialInstallSource({
      current: inspect({
        path: "/Applications/ChatGPT.app",
        patched: true,
        installId,
        listing: originalA,
      }),
      backup: inspect({
        path: "ChatGPT.app.pre-live",
        patched: false,
        identity: { ...originalA, asarFileHash: "tampered" },
      }),
      record: recordFor(originalA),
    });
    expect(decision).toEqual({
      action: "reject",
      reason: "original backup does not match this install record",
    });
  });
});

describe("canRestoreOfficial", () => {
  test("refuses to restore when the current app is no longer marked", () => {
    const decision = canRestoreOfficial({
      current: inspect({
        path: "/Applications/ChatGPT.app",
        patched: false,
        listing: originalB,
        identity: originalB,
      }),
      backup: inspect({
        path: "ChatGPT.app.pre-live",
        patched: false,
        identity: originalA,
      }),
      record: recordFor(originalA),
    });
    expect(decision).toEqual({ ok: false, reason: NOT_INCODEX_INSTALL });
  });

  test("refuses to restore after an official upgrade that kept an old backup", () => {
    const decision = canRestoreOfficial({
      current: inspect({
        path: "/Applications/ChatGPT.app",
        patched: false,
        listing: originalB,
        identity: originalB,
      }),
      backup: inspect({
        path: "ChatGPT.app.pre-live",
        patched: false,
        identity: originalA,
      }),
      record: recordFor(originalA),
    });
    expect(decision.ok).toBe(false);
    if (decision.ok) throw new Error("expected refuse");
    expect(decision.reason).toBe(NOT_INCODEX_INSTALL);
  });

  test("allows restore only when the current marker and original backup match the install record", () => {
    const decision = canRestoreOfficial({
      current: inspect({
        path: "/Applications/ChatGPT.app",
        patched: true,
        installId,
        listing: originalA,
      }),
      backup: inspect({
        path: "ChatGPT.app.pre-live",
        patched: false,
        identity: originalA,
      }),
      record: recordFor(originalA),
    });
    expect(decision).toEqual({ ok: true });
  });

  test("does not treat an unpatched-looking backup as enough evidence to restore a different version", () => {
    const decision = officialBackupMatchesInstall({
      current: inspect({
        path: "/Applications/ChatGPT.app",
        patched: true,
        installId,
        listing: originalB,
      }),
      backup: inspect({
        path: "ChatGPT.app.pre-live",
        patched: false,
        identity: originalA,
      }),
      record: null,
    });
    expect(decision).toEqual({ ok: false, reason: NOT_INCODEX_INSTALL });
  });

  test("pre-P0.1 installs may restore only a same-version original, never an older backup", () => {
    expect(
      officialBackupMatchesInstall({
        current: inspect({
          path: "/Applications/ChatGPT.app",
          patched: true,
          listing: originalA,
        }),
        backup: inspect({
          path: "ChatGPT.app.pre-live",
          patched: false,
          identity: originalA,
        }),
        record: null,
      }),
    ).toEqual({ ok: true });
    expect(
      canRestoreOfficial({
        current: inspect({
          path: "/Applications/ChatGPT.app",
          patched: true,
          listing: originalA,
        }),
        backup: inspect({
          path: "ChatGPT.app.pre-live",
          patched: false,
          identity: originalB,
        }),
        record: null,
      }),
    ).toEqual({ ok: false, reason: NOT_INCODEX_INSTALL });
  });
});
