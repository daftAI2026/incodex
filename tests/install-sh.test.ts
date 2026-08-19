import { describe, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";

const repo = join(import.meta.dir, "..");
const installSh = join(repo, "install.sh");

function sha256(path: string): string {
  const hashed = spawnSync("shasum", ["-a", "256", path], { encoding: "utf8" });
  if (hashed.status !== 0) throw new Error(hashed.stderr || "shasum failed");
  return hashed.stdout.trim().split(/\s+/)[0] ?? "";
}

function writePayload(dir: string, name: string, body: string): string {
  const path = join(dir, name);
  writeFileSync(path, body);
  chmodSync(path, 0o755);
  return path;
}

describe("install.sh", () => {
  test("exists at the repo root", () => {
    expect(existsSync(installSh)).toBe(true);
  });

  test("never patches Codex; it only installs the CLI", () => {
    const text = readFileSync(installSh, "utf8");
    expect(text).not.toMatch(/(?:^|[\s"`$])incodex(?:\s+|")install\b/);
    expect(text).not.toContain("--confirm-live");
    expect(text).toContain("SHA256SUMS");
  });

  test("installs incodex and inc from a verified local release dir", () => {
    const release = mkdtempSync(join(tmpdir(), "incodex-rel-"));
    const prefix = mkdtempSync(join(tmpdir(), "incodex-pre-"));
    writePayload(release, "incodex-darwin-arm64", "#!/bin/sh\necho fake-cli\n");
    writePayload(release, "incodex-darwin-x64", "#!/bin/sh\necho fake-cli\n");
    const sums = ["incodex-darwin-arm64", "incodex-darwin-x64"]
      .map((name) => `${sha256(join(release, name))}  ${name}`)
      .join("\n");
    writeFileSync(join(release, "SHA256SUMS"), `${sums}\n`);

    const ran = spawnSync("bash", [installSh], {
      encoding: "utf8",
      env: {
        ...process.env,
        INCODEX_DOWNLOAD_DIR: release,
        INCODEX_PREFIX: prefix,
        INCODEX_ARCH: "arm64",
      },
    });
    expect(ran.status).toBe(0);
    expect(ran.stdout + ran.stderr).not.toMatch(/\bincodex install\b/);

    const dest = join(prefix, "bin", "incodex");
    const alias = join(prefix, "bin", "inc");
    expect(existsSync(dest)).toBe(true);
    expect(existsSync(alias)).toBe(true);
    const probe = spawnSync(dest, [], { encoding: "utf8" });
    expect(probe.stdout).toContain("fake-cli");
  });

  test("refuses to install when SHA256SUMS is missing", () => {
    const release = mkdtempSync(join(tmpdir(), "incodex-rel-"));
    const prefix = mkdtempSync(join(tmpdir(), "incodex-pre-"));
    writePayload(release, "incodex-darwin-arm64", "#!/bin/sh\necho fake-cli\n");

    const ran = spawnSync("bash", [installSh], {
      encoding: "utf8",
      env: {
        ...process.env,
        INCODEX_DOWNLOAD_DIR: release,
        INCODEX_PREFIX: prefix,
        INCODEX_ARCH: "arm64",
      },
    });
    expect(ran.status).not.toBe(0);
    expect(ran.stdout + ran.stderr).toMatch(/SHA256SUMS/);
    expect(existsSync(join(prefix, "bin", "incodex"))).toBe(false);
  });

  test("refuses to install when the checksum does not match", () => {
    const release = mkdtempSync(join(tmpdir(), "incodex-rel-"));
    const prefix = mkdtempSync(join(tmpdir(), "incodex-pre-"));
    writePayload(release, "incodex-darwin-arm64", "#!/bin/sh\necho fake-cli\n");
    writeFileSync(join(release, "SHA256SUMS"), `${"0".repeat(64)}  incodex-darwin-arm64\n`);

    const ran = spawnSync("bash", [installSh], {
      encoding: "utf8",
      env: {
        ...process.env,
        INCODEX_DOWNLOAD_DIR: release,
        INCODEX_PREFIX: prefix,
        INCODEX_ARCH: "arm64",
      },
    });
    expect(ran.status).not.toBe(0);
    expect(existsSync(join(prefix, "bin", "incodex"))).toBe(false);
  });
});
