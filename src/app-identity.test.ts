import { createHash } from "node:crypto";
import { describe, expect, test } from "bun:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  fileSha256,
  identitiesEqual,
  isCompleteIdentity,
  listingsEqual,
} from "./app-identity";

describe("identity helpers", () => {
  test("fileSha256 hashes the full file, not a prefix", () => {
    const dir = mkdtempSync(join(tmpdir(), "incodex-hash-"));
    const path = join(dir, "blob");
    writeFileSync(path, "incodex-identity");
    expect(fileSha256(path)).toBe(createHash("sha256").update("incodex-identity").digest("hex"));
  });

  test("listingsEqual ignores hashes and identitiesEqual does not", () => {
    const left = {
      bundleIdentifier: "com.openai.codex",
      appVersion: "1.0.0",
      appBuild: "100",
      architecture: "arm64",
      asarFileHash: "aaa",
      plistFileHash: "bbb",
    };
    const right = { ...left, asarFileHash: "ccc" };
    expect(listingsEqual(left, right)).toBe(true);
    expect(identitiesEqual(left, right)).toBe(false);
    expect(isCompleteIdentity(left)).toBe(true);
    expect(isCompleteIdentity({ ...left, architecture: "" })).toBe(false);
  });
});
