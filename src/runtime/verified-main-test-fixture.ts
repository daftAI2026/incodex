import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

export function verifiedMainFixture() {
  const dist = join(import.meta.dir, "../../dist");
  const manifestBytes = readFileSync(join(dist, "runtime-manifest.json"));
  const manifest = JSON.parse(manifestBytes.toString("utf8"));
  const manifestSha256 = createHash("sha256").update(manifestBytes).digest("hex");
  const home = mkdtempSync(join(tmpdir(), "incodex-verified-main-test-"));
  const runtimeRoot = join(home, ".incodex", "runtime");
  const release = `releases/${manifest.runtimeVersion}-${manifestSha256}`;
  const releaseDir = join(runtimeRoot, release);
  mkdirSync(releaseDir, { recursive: true });
  for (const name of Object.keys(manifest.files)) {
    copyFileSync(join(dist, name), join(releaseDir, name));
  }
  copyFileSync(join(dist, "runtime-manifest.json"), join(releaseDir, "runtime-manifest.json"));
  writeFileSync(join(runtimeRoot, "current.json"), JSON.stringify({
    schemaVersion: 1,
    version: manifest.runtimeVersion,
    release,
    manifestSha256,
    sourceCommit: manifest.sourceCommit,
    files: manifest.files,
  }));
  return { home, releaseDir, mainPath: join(releaseDir, "incodex-main.cjs") };
}

export function withVerifiedMainFixture<T>(run: (fixture: ReturnType<typeof verifiedMainFixture>) => T): T {
  const fixture = verifiedMainFixture();
  const priorHome = process.env.HOME;
  process.env.HOME = fixture.home;
  try {
    return run(fixture);
  } finally {
    if (priorHome === undefined) delete process.env.HOME;
    else process.env.HOME = priorHome;
  }
}

export function loadVerifiedMainFixture(): Record<string, unknown> {
  return withVerifiedMainFixture(({ mainPath }) => createRequire(import.meta.url)(mainPath));
}
