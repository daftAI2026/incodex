import { describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  EXTERNAL_RUNTIME_FILES,
  publishExternalRuntime,
  resolveExternalMain,
  verifyExternalRuntime,
} from "./external-runtime";

function artifacts(): Record<string, string> {
  const files: Record<string, string> = {};
  for (const name of EXTERNAL_RUNTIME_FILES) files[name] = `// ${name}\n`;
  return files;
}

describe("external runtime", () => {
  test("publish writes hashed files and current.json without symlinks", () => {
    const userRoot = mkdtempSync(join(tmpdir(), "incodex-rt-"));
    const current = publishExternalRuntime({ userRoot, version: "0.1.0", files: artifacts() });
    expect(current.schemaVersion).toBe(1);
    expect(current.release).toBe("releases/0.1.0");
    expect(current.files["incodex-main.cjs"]).toMatch(/^[0-9a-f]{64}$/);
    const verified = verifyExternalRuntime(userRoot);
    expect(verified.main).toBe(join(userRoot, "runtime/releases/0.1.0/incodex-main.cjs"));
    expect(readFileSync(verified.main, "utf8")).toBe("// incodex-main.cjs\n");
  });

  test("a wrong hash refuses to resolve the main", () => {
    const home = mkdtempSync(join(tmpdir(), "incodex-home-"));
    const userRoot = join(home, ".incodex");
    publishExternalRuntime({ userRoot, version: "0.1.0", files: artifacts() });
    writeFileSync(join(userRoot, "runtime/releases/0.1.0/incodex-main.cjs"), "tampered\n");
    expect(() => verifyExternalRuntime(userRoot)).toThrow(/hash mismatch/);
    expect(() => resolveExternalMain({ HOME: home })).toThrow(/hash mismatch/);
  });

  test("missing HOME fails open instead of a relative path", () => {
    expect(() => resolveExternalMain({})).toThrow(/HOME/);
    expect(() => resolveExternalMain({ HOME: "" })).toThrow(/HOME/);
  });

  test("missing current.json fails", () => {
    const home = mkdtempSync(join(tmpdir(), "incodex-home-"));
    mkdirSync(join(home, ".incodex"), { recursive: true });
    expect(() => resolveExternalMain({ HOME: home })).toThrow();
  });

  test("refuses a symlink runtime root", () => {
    const userRoot = mkdtempSync(join(tmpdir(), "incodex-rt-"));
    const other = mkdtempSync(join(tmpdir(), "incodex-rt-other-"));
    symlinkSync(other, join(userRoot, "runtime"));
    expect(() => publishExternalRuntime({ userRoot, version: "0.1.0", files: artifacts() })).toThrow(/symlink/);
  });

  test("DEV_HOT can load a target override without current.json", () => {
    const home = mkdtempSync(join(tmpdir(), "incodex-home-"));
    const exec = "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT";
    const { createHash } = require("node:crypto") as typeof import("node:crypto");
    const id = createHash("sha256").update(exec).digest("hex").slice(0, 12);
    const dest = join(home, ".incodex", "targets", id);
    mkdirSync(dest, { recursive: true });
    writeFileSync(join(dest, "incodex-main.cjs"), "hot\n");
    expect(resolveExternalMain({ HOME: home, INCODEX_DEV_HOT: "1" }, exec)).toBe(join(dest, "incodex-main.cjs"));
  });

  test("replacing a version uses rename and leaves a valid current.json", () => {
    const userRoot = mkdtempSync(join(tmpdir(), "incodex-rt-"));
    publishExternalRuntime({ userRoot, version: "0.1.0", files: artifacts() });
    const next = { ...artifacts(), "incodex-main.cjs": "// next\n" };
    publishExternalRuntime({ userRoot, version: "0.1.0", files: next });
    expect(readFileSync(verifyExternalRuntime(userRoot).main, "utf8")).toBe("// next\n");
  });
});
