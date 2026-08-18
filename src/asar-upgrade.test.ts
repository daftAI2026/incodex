import { describe, expect, test } from "bun:test";
import { createPackageWithOptions, extractAll, extractFile } from "@electron/asar";
import { mkdirSync, mkdtempSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function tmp(): string {
  return mkdtempSync(join(tmpdir(), "incodex-asar4-"));
}

describe("@electron/asar 4 robustness", () => {
  test("refuses a cyclic symlink when packing", async () => {
    const root = tmp();
    const src = join(root, "src");
    mkdirSync(src, { recursive: true });
    writeFileSync(join(src, "package.json"), `${JSON.stringify({ main: "index.js" })}\n`);
    writeFileSync(join(src, "index.js"), "ok\n");
    symlinkSync(src, join(src, "loop"));
    await expect(createPackageWithOptions(src, join(root, "app.asar"), {})).rejects.toThrow();
  });

  test("refuses a directory-traversal entry when packing", async () => {
    const root = tmp();
    const src = join(root, "src");
    mkdirSync(src, { recursive: true });
    writeFileSync(join(src, "package.json"), `${JSON.stringify({ main: "index.js" })}\n`);
    writeFileSync(join(src, "index.js"), "ok\n");
    symlinkSync(join(root, "..", "outside"), join(src, "escape"));
    await expect(createPackageWithOptions(src, join(root, "app.asar"), {})).rejects.toThrow();
  });

  test("extractFile throws on a truncated / corrupt archive", () => {
    const file = join(tmp(), "bad.asar");
    writeFileSync(file, "not-an-asar");
    expect(() => extractFile(file, "package.json")).toThrow();
  });

  test("extractAll throws on an archive with a nonsense header", () => {
    const file = join(tmp(), "bad.asar");
    const huge = Buffer.alloc(1024, 0xff);
    writeFileSync(file, huge);
    expect(() => extractAll(file, join(tmp(), "out"))).toThrow();
  });
});
