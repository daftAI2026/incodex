import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, test } from "bun:test";
import { cliVersion } from "./cli-version";

const root = join(import.meta.dir, "..");
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as { version: string };

describe("cliVersion", () => {
  test("matches package.json", () => {
    expect(cliVersion()).toBe(pkg.version);
  });

  test("does not read package.json from disk at runtime", () => {
    const src = readFileSync(join(import.meta.dir, "cli-version.ts"), "utf8");
    expect(src).not.toContain("readFileSync");
    expect(src).toContain("package.json");
  });

  test("a compiled binary prints the version without a sibling package.json", () => {
    const dir = mkdtempSync(join(tmpdir(), "incodex-cli-version-"));
    const entry = join(dir, "entry.ts");
    const outfile = join(dir, "incodex-version");
    writeFileSync(
      entry,
      `import { cliVersion } from ${JSON.stringify(join(import.meta.dir, "cli-version.ts"))};\nconsole.log(cliVersion());\n`,
    );
    const built = spawnSync("bun", ["build", entry, "--compile", "--outfile", outfile], {
      encoding: "utf8",
    });
    expect(built.status).toBe(0);
    chmodSync(outfile, 0o755);
    const ran = spawnSync(outfile, [], { encoding: "utf8", cwd: dir });
    expect(ran.status).toBe(0);
    expect(ran.stderr).not.toContain("package.json");
    expect(ran.stdout.trim()).toBe(pkg.version);
  });
});
