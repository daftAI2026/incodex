import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { createPackageWithOptions, extractFile, listPackage, uncache } from "@electron/asar";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { collectUnpackOptions, headerHash, patchAsar, readPackageMain } from "./asar";
import { LOADER_NAME, MARKER_KEY } from "./paths";

const runtime = {
  loaderSource: "/* loader */",
  injectSource: "/* inject */",
  mainSource: "/* main */",
  preloadSource: "/* preload */",
  safeHomeSource: "/* safe */",
  ipcGuardSource: "/* ipc */",
  instanceSource: "/* instance */",
  runtimeLoadSource: "/* load */",
  windowKindSource: "/* window */",
};

function fixture(): string {
  return mkdtempSync(join(tmpdir(), "incodex-fix-"));
}

async function pack(files: Record<string, string>): Promise<string> {
  const root = fixture();
  const src = join(root, "src");
  mkdirSync(src, { recursive: true });
  for (const [rel, body] of Object.entries(files)) {
    const dest = join(src, rel);
    mkdirSync(join(dest, ".."), { recursive: true });
    writeFileSync(dest, body);
  }
  const archive = join(root, "app.asar");
  await createPackageWithOptions(src, archive, {});
  return archive;
}

describe("ASAR fixtures", () => {
  test("package.json with no main is refused", async () => {
    const archive = await pack({ "package.json": "{}\n", "index.js": "ok\n" });
    expect(readPackageMain(archive).main).toBe("");
    await expect(patchAsar({ asarPath: archive, ...runtime })).rejects.toThrow(/no main/);
  });

  test("package.json main pointing at a missing file is still recorded", async () => {
    const archive = await pack({
      "package.json": `${JSON.stringify({ main: "missing.js" })}\n`,
    });
    expect(readPackageMain(archive).main).toBe("missing.js");
  });

  test("paths with spaces survive a rebuild", async () => {
    const archive = await pack({
      "package.json": `${JSON.stringify({ main: "index.js" })}\n`,
      "index.js": "ok\n",
      "assets/my file.txt": "hello\n",
    });
    await patchAsar({ asarPath: archive, installId: "space", ...runtime });
    expect(extractFile(archive, "assets/my file.txt").toString("utf8")).toBe("hello\n");
  });

  test("already patched archives keep the original main", async () => {
    const archive = await pack({
      "package.json": `${JSON.stringify({ main: "index.js" })}\n`,
      "index.js": "ok\n",
    });
    await patchAsar({ asarPath: archive, installId: "one", ...runtime });
    const first = readPackageMain(archive);
    expect(first.alreadyPatched).toBe(true);
    expect(first.main).toBe("index.js");
    await patchAsar({ asarPath: archive, installId: "two", ...runtime });
    expect(readPackageMain(archive).main).toBe("index.js");
  });

  test("a fully packed archive keeps file hashes, lists, marker, and header integrity", async () => {
    const archive = await pack({
      "package.json": `${JSON.stringify({ main: "index.js" })}\n`,
      "index.js": "module.exports = 1\n",
      "lib/util.js": "exports.ok = true\n",
    });
    const before = snapshot(archive);
    const patched = await patchAsar({ asarPath: archive, installId: "pack", ...runtime });
    uncache(archive);
    expect(patched.originalMain).toBe("index.js");
    expect(patched.hash).toMatch(/^[0-9a-f]{64}$/);
    expect(patched.hash).toBe(headerHash(archive));
    expect(sha(extractFile(archive, "index.js"))).toBe(before.hashes["/index.js"]);
    expect(sha(extractFile(archive, "lib/util.js"))).toBe(before.hashes["/lib/util.js"]);
    const listed = listPackage(archive, { isPack: false });
    expect(listed).toContain("/index.js");
    expect(listed).toContain("/lib/util.js");
    expect(listed).toContain(`/${LOADER_NAME}`);
    const pkg = JSON.parse(extractFile(archive, "package.json").toString("utf8")) as {
      main: string;
      [MARKER_KEY]?: { originalMain?: string; installId?: string };
    };
    expect(pkg.main).toBe(LOADER_NAME);
    expect(pkg[MARKER_KEY]).toEqual({ originalMain: "index.js", installId: "pack" });
  });

  test("an unpacked directory is still unpacked after rebuild", async () => {
    const root = fixture();
    const src = join(root, "src");
    mkdirSync(join(src, "native"), { recursive: true });
    writeFileSync(join(src, "package.json"), `${JSON.stringify({ main: "index.js" })}\n`);
    writeFileSync(join(src, "index.js"), "ok\n");
    writeFileSync(join(src, "native", "addon.node"), "binary");
    writeFileSync(join(src, "native", "helper.bin"), "help");
    const archive = join(root, "app.asar");
    await createPackageWithOptions(src, archive, { unpackDir: "native" });
    expect(existsSync(join(`${archive}.unpacked`, "native", "addon.node"))).toBe(true);
    await patchAsar({ asarPath: archive, installId: "dir", ...runtime });
    expect(readFileSync(join(`${archive}.unpacked`, "native", "addon.node"), "utf8")).toBe("binary");
    expect(readFileSync(join(`${archive}.unpacked`, "native", "helper.bin"), "utf8")).toBe("help");
    const unpack = collectUnpackOptions(archive);
    expect(unpack.unpackDir).toContain("native");
  });

  test("an internal symlink survives rebuild", async () => {
    const root = fixture();
    const src = join(root, "src");
    mkdirSync(src, { recursive: true });
    writeFileSync(join(src, "package.json"), `${JSON.stringify({ main: "index.js" })}\n`);
    writeFileSync(join(src, "index.js"), "ok\n");
    writeFileSync(join(src, "target.txt"), "linked\n");
    symlinkSync("target.txt", join(src, "alias.txt"));
    const archive = join(root, "app.asar");
    await createPackageWithOptions(src, archive, {});
    await patchAsar({ asarPath: archive, installId: "link", ...runtime });
    expect(extractFile(archive, "target.txt").toString("utf8")).toBe("linked\n");
    expect(extractFile(archive, "alias.txt").toString("utf8")).toBe("linked\n");
  });

  test("an injected filename already in the archive is overwritten", async () => {
    const archive = await pack({
      "package.json": `${JSON.stringify({ main: "index.js" })}\n`,
      "index.js": "ok\n",
      [LOADER_NAME]: "OLD LOADER\n",
    });
    expect(extractFile(archive, LOADER_NAME).toString("utf8")).toBe("OLD LOADER\n");
    await patchAsar({ asarPath: archive, installId: "clash", ...runtime });
    expect(extractFile(archive, LOADER_NAME).toString("utf8")).toBe("/* loader */");
    expect(extractFile(archive, "index.js").toString("utf8")).toBe("ok\n");
    expect(readPackageMain(archive).main).toBe("index.js");
  });

  test("a nonsense file offset is refused", () => {
    const archive = join(fixture(), "bad-offset.asar");
    const validHeader = JSON.stringify({ files: { "index.js": { size: 2, offset: "99999999" } } });
    writeFileSync(archive, Buffer.concat([Buffer.from("xxxx"), Buffer.from(validHeader), Buffer.from("no")]));
    expect(() => extractFile(archive, "index.js")).toThrow();
  });

  test("a multi-megabyte archive still rebuilds with stable content hashes", async () => {
    const big = "A".repeat(2 * 1024 * 1024);
    const archive = await pack({
      "package.json": `${JSON.stringify({ main: "index.js" })}\n`,
      "index.js": "ok\n",
      "blob.bin": big,
    });
    const before = sha(extractFile(archive, "blob.bin"));
    await patchAsar({ asarPath: archive, installId: "big", ...runtime });
    expect(sha(extractFile(archive, "blob.bin"))).toBe(before);
    expect(extractFile(archive, "index.js").toString("utf8")).toBe("ok\n");
    expect(readPackageMain(archive).main).toBe("index.js");
  });
});

function sha(buf: Buffer): string {
  return createHash("sha256").update(buf).digest("hex");
}

function snapshot(archive: string): { files: string[]; hashes: Record<string, string> } {
  const files = listPackage(archive, { isPack: false });
  const hashes: Record<string, string> = {};
  for (const file of files) {
    if (file.endsWith("/")) continue;
    try {
      hashes[file] = sha(extractFile(archive, file.replace(/^\//, "")));
    } catch {
      /* directories or links */
    }
  }
  return { files, hashes };
}
