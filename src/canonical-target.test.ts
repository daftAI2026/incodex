import { describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, realpathSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { canonicalize, canonicalPath, isOfficialApp } from "./canonical-target";

function scratch(): string {
  return mkdtempSync(join(tmpdir(), "incodex-canon-"));
}

describe("canonical target", () => {
  test("a parent symlink into the official directory is still the official app", () => {
    const root = scratch();
    const applications = join(root, "Applications");
    const official = join(applications, "ChatGPT.app");
    mkdirSync(official, { recursive: true });
    const aliasParent = join(root, "apps");
    symlinkSync(applications, aliasParent);
    const aliased = join(aliasParent, "ChatGPT.app");

    const target = canonicalize(aliased, official);
    expect(target.realPath).toBe(realpathSync.native(official));
    expect(target.isOfficial).toBe(true);
    expect(isOfficialApp(aliased, official)).toBe(true);
  });

  test("lexical resolve() is not enough: .. is collapsed, but a symlink is not", () => {
    const root = scratch();
    const official = join(root, "Applications", "ChatGPT.app");
    mkdirSync(official, { recursive: true });
    expect(canonicalPath(join(root, "Applications", "..", "Applications", "ChatGPT.app"))).toBe(
      realpathSync.native(official),
    );
    const aliasParent = join(root, "apps");
    symlinkSync(join(root, "Applications"), aliasParent);
    expect(canonicalPath(join(aliasParent, "ChatGPT.app"))).toBe(realpathSync.native(official));
  });

  test("a real custom app is not official", () => {
    const root = scratch();
    const official = join(root, "Applications", "ChatGPT.app");
    const custom = join(root, "scratch", "ChatGPT.app");
    mkdirSync(official, { recursive: true });
    mkdirSync(custom, { recursive: true });
    expect(canonicalize(custom, official).isOfficial).toBe(false);
  });
});
