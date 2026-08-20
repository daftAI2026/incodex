import { describe, expect, test } from "bun:test";
import { chmodSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
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

function writeLegacyBunLauncher(prefix: string): string {
  const bin = join(prefix, "bin");
  const source = join(prefix, "legacy-launcher.c");
  const executable = join(bin, "incodex");
  mkdirSync(bin, { recursive: true });
  writeFileSync(
    source,
    `#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

int main(int argc, char **argv) {
  if (argc != 2) return 64;
  pid_t child = fork();
  if (child == 0) {
    execl("/bin/bash", "bash", "-lc", "bash \\"$1\\"", "legacy", argv[1], (char *)0);
    return 127;
  }
  if (child < 0) return 71;
  int status = 0;
  if (waitpid(child, &status, 0) < 0) return 71;
  return WIFEXITED(status) ? WEXITSTATUS(status) : 128;
}
`,
  );
  const compiled = spawnSync("/usr/bin/cc", [source, "-o", executable], { encoding: "utf8" });
  if (compiled.status !== 0) throw new Error(compiled.stderr || "legacy launcher compile failed");
  return executable;
}

function runLegacyBunUpdate(release: string, prefix: string, home: string) {
  const executable = writeLegacyBunLauncher(prefix);
  return spawnSync(executable, [installSh], {
    encoding: "utf8",
    env: {
      ...process.env,
      HOME: home,
      INCODEX_DOWNLOAD_DIR: release,
      INCODEX_PREFIX: "/$bunfs/root",
      INCODEX_ARCH: "arm64",
    },
  });
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

  test("recovers the default prefix passed by a legacy Bun standalone update", () => {
    const release = mkdtempSync(join(tmpdir(), "incodex-rel-"));
    const home = mkdtempSync(join(tmpdir(), "incodex-home-"));
    const prefix = join(home, ".local");
    writePayload(release, "incodex-darwin-arm64", "#!/bin/sh\necho fake-cli\n");
    const sums = `${sha256(join(release, "incodex-darwin-arm64"))}  incodex-darwin-arm64\n`;
    writeFileSync(join(release, "SHA256SUMS"), sums);

    const ran = runLegacyBunUpdate(release, prefix, home);

    expect(ran.status).toBe(0);
    expect(existsSync(join(home, ".local", "bin", "incodex"))).toBe(true);
    expect(existsSync(join(home, ".local", "bin", "inc"))).toBe(true);
    expect(existsSync("/$bunfs/root/bin/incodex")).toBe(false);
  });

  test("recovers a custom prefix from the legacy Bun updater process", () => {
    const release = mkdtempSync(join(tmpdir(), "incodex-rel-"));
    const home = mkdtempSync(join(tmpdir(), "incodex-home-"));
    const prefix = mkdtempSync(join(tmpdir(), "incodex-custom-"));
    writePayload(release, "incodex-darwin-arm64", "#!/bin/sh\necho fake-cli\n");
    const sums = `${sha256(join(release, "incodex-darwin-arm64"))}  incodex-darwin-arm64\n`;
    writeFileSync(join(release, "SHA256SUMS"), sums);

    const ran = runLegacyBunUpdate(release, prefix, home);

    expect(ran.status).toBe(0);
    expect(existsSync(join(prefix, "bin", "incodex"))).toBe(true);
    expect(existsSync(join(prefix, "bin", "inc"))).toBe(true);
    expect(existsSync(join(home, ".local", "bin", "incodex"))).toBe(false);
  });

  test("refuses an unverifiable legacy Bun prefix instead of silently relocating it", () => {
    const release = mkdtempSync(join(tmpdir(), "incodex-rel-"));
    const home = mkdtempSync(join(tmpdir(), "incodex-home-"));
    writePayload(release, "incodex-darwin-arm64", "#!/bin/sh\necho fake-cli\n");
    const sums = `${sha256(join(release, "incodex-darwin-arm64"))}  incodex-darwin-arm64\n`;
    writeFileSync(join(release, "SHA256SUMS"), sums);

    const ran = spawnSync("bash", [installSh], {
      encoding: "utf8",
      env: {
        ...process.env,
        HOME: home,
        INCODEX_DOWNLOAD_DIR: release,
        INCODEX_PREFIX: "/$bunfs/root",
        INCODEX_ARCH: "arm64",
      },
    });

    expect(ran.status).not.toBe(0);
    expect(ran.stderr).toContain("legacy Bun update prefix");
    expect(existsSync(join(home, ".local", "bin", "incodex"))).toBe(false);
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

  test("does not fall back to a legacy Bun asset when the stable Rust asset is missing", () => {
    const release = mkdtempSync(join(tmpdir(), "incodex-rel-"));
    const prefix = mkdtempSync(join(tmpdir(), "incodex-pre-"));
    const legacy = "incodex-darwin-arm64-legacy-bun";
    writePayload(release, legacy, "#!/bin/sh\necho legacy-bun\n");
    writeFileSync(join(release, "SHA256SUMS"), `${sha256(join(release, legacy))}  ${legacy}\n`);

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
    expect(ran.stdout + ran.stderr).toContain("missing incodex-darwin-arm64");
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

  test("keeps the existing CLI when the staged payload reports the wrong version", () => {
    const release = mkdtempSync(join(tmpdir(), "incodex-rel-"));
    const prefix = mkdtempSync(join(tmpdir(), "incodex-pre-"));
    const bin = join(prefix, "bin");
    mkdirSync(bin, { recursive: true });
    writePayload(bin, "incodex", "#!/bin/sh\nprintf '%s\\n' 'Incodex version 0.2.0'\n");
    symlinkSync("incodex", join(bin, "inc"));
    writePayload(
      release,
      "incodex-darwin-arm64",
      "#!/bin/sh\nprintf '%s\\n' 'Incodex version 8.8.8'\n",
    );
    writeFileSync(
      join(release, "SHA256SUMS"),
      `${sha256(join(release, "incodex-darwin-arm64"))}  incodex-darwin-arm64\n`,
    );

    const ran = spawnSync("bash", [installSh], {
      encoding: "utf8",
      env: {
        ...process.env,
        INCODEX_DOWNLOAD_DIR: release,
        INCODEX_PREFIX: prefix,
        INCODEX_ARCH: "arm64",
        INCODEX_EXPECTED_VERSION: "9.9.9",
      },
    });

    expect(ran.status).not.toBe(0);
    expect(ran.stderr).toContain("does not report expected version 9.9.9");
    expect(readFileSync(join(bin, "incodex"), "utf8")).toContain("0.2.0");
    expect(lstatSync(join(bin, "inc")).isSymbolicLink()).toBe(true);
  });

  test("atomically replaces an incodex symlink without modifying its target", () => {
    const release = mkdtempSync(join(tmpdir(), "incodex-rel-"));
    const prefix = mkdtempSync(join(tmpdir(), "incodex-pre-"));
    const outside = mkdtempSync(join(tmpdir(), "incodex-outside-"));
    const bin = join(prefix, "bin");
    const victim = writePayload(outside, "victim", "#!/bin/sh\necho do-not-touch\n");
    mkdirSync(bin, { recursive: true });
    symlinkSync(victim, join(bin, "incodex"));
    writePayload(
      release,
      "incodex-darwin-arm64",
      "#!/bin/sh\nprintf '%s\\n' 'Incodex version 9.9.9'\n",
    );
    writeFileSync(
      join(release, "SHA256SUMS"),
      `${sha256(join(release, "incodex-darwin-arm64"))}  incodex-darwin-arm64\n`,
    );

    const ran = spawnSync("bash", [installSh], {
      encoding: "utf8",
      env: {
        ...process.env,
        INCODEX_DOWNLOAD_DIR: release,
        INCODEX_PREFIX: prefix,
        INCODEX_ARCH: "arm64",
        INCODEX_EXPECTED_VERSION: "9.9.9",
      },
    });

    expect(ran.status).toBe(0);
    expect(readFileSync(victim, "utf8")).toContain("do-not-touch");
    expect(lstatSync(join(bin, "incodex")).isSymbolicLink()).toBe(false);
    expect(readFileSync(join(bin, "incodex"), "utf8")).toContain("9.9.9");
  });

  test("checksum failure leaves an existing CLI untouched", () => {
    const release = mkdtempSync(join(tmpdir(), "incodex-rel-"));
    const prefix = mkdtempSync(join(tmpdir(), "incodex-pre-"));
    const bin = join(prefix, "bin");
    mkdirSync(bin, { recursive: true });
    writePayload(bin, "incodex", "#!/bin/sh\necho old-cli\n");
    writePayload(release, "incodex-darwin-arm64", "#!/bin/sh\necho new-cli\n");
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
    expect(readFileSync(join(bin, "incodex"), "utf8")).toContain("old-cli");
  });
});
