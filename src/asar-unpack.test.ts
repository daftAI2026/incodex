import { describe, expect, test } from "bun:test";
import { createPackageWithOptions } from "@electron/asar";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { collectUnpackOptions, escapeGlob, exactUnpackPattern, patchAsar } from "./asar";

function fixtureDir(): string {
  return mkdtempSync(join(tmpdir(), "incodex-asar-"));
}

describe("unpack glob escaping", () => {
  test("escapes glob metacharacters instead of prefixing **/", () => {
    expect(escapeGlob("foo[bar].node")).toBe("foo\\[bar\\].node");
    expect(exactUnpackPattern("native/foo*.node")).toBe("native/foo\\*.node");
    expect(exactUnpackPattern("/hidapi/libhidapi-*.dylib")).toBe("hidapi/libhidapi-\\*.dylib");
  });
});

describe("unpack fixtures", () => {
  test("rebuilt unpack set matches the original header for a packed+unpacked archive", async () => {
    const root = fixtureDir();
    const src = join(root, "src");
    mkdirSync(join(src, "native"), { recursive: true });
    writeFileSync(join(src, "package.json"), `${JSON.stringify({ main: "index.js" }, null, 2)}\n`);
    writeFileSync(join(src, "index.js"), "module.exports = 1\n");
    writeFileSync(join(src, "native", "addon.node"), "binary");
    writeFileSync(join(src, "native", "foo[bar].node"), "special");
    writeFileSync(join(src, "native", "kept.js"), "packed");
    const archive = join(root, "app.asar");
    await createPackageWithOptions(src, archive, { unpack: "**/*.node" });
    expect(existsSync(`${archive}.unpacked`)).toBe(true);

    const options = collectUnpackOptions(archive);
    expect(options.unpack).toContain("**/native/addon.node");
    expect(options.unpack).toContain("foo\\[bar\\].node");
    expect(options.unpack?.includes("**/native/foo\\[bar\\].node")).toBe(true);

    await patchAsar({
      asarPath: archive,
      loaderSource: "/* loader */",
      installId: "test",
    });
    expect(existsSync(`${archive}.unpacked`)).toBe(true);
    expect(readFileSync(join(`${archive}.unpacked`, "native", "addon.node"), "utf8")).toBe("binary");
    const after = collectUnpackOptions(archive);
    expect(after.unpack).toContain("**/native/addon.node");
    expect(after.unpack).toContain("**/native/foo\\[bar\\].node");
  });
});
