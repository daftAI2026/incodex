import { describe, expect, test } from "bun:test";
import { createPackageWithOptions, extractFile } from "@electron/asar";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { patchAsar, readPackageMain } from "./asar";

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
});
