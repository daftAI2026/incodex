import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

describe("Electron session identity contract", () => {
  test("parent and child burn paths carry the created root identity", () => {
    const source = readFileSync(join(import.meta.dir, "runtime/incodex-main.cts"), "utf8");
    expect(source).toContain("INCODEX_SESSION_INO");
    expect(source).toContain("INCODEX_SESSION_DEV");
    expect(source).toContain("ino: session.ino");
    expect(source).toContain("dev: session.dev");
  });
});
