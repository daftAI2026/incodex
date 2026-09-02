import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const runtimeManifestPath = join(root, "dist/runtime-manifest.json");

function committedSourceCommit(): string {
  const manifest = JSON.parse(readFileSync(runtimeManifestPath, "utf8")) as {
    sourceCommit?: unknown;
  };
  return typeof manifest.sourceCommit === "string" ? manifest.sourceCommit : "";
}

function main(): void {
  const built = spawnSync("bun", ["src/build-runtime.ts"], {
    cwd: root,
    encoding: "utf8",
    stdio: "inherit",
    env: {
      ...process.env,
      SOURCE_COMMIT: process.env.SOURCE_COMMIT || committedSourceCommit(),
    },
  });
  if (built.status !== 0) {
    process.exit(built.status ?? 1);
  }

  const diff = spawnSync("git", ["diff", "--exit-code", "--", "dist"], {
    cwd: root,
    encoding: "utf8",
  });
  if (diff.status !== 0) {
    process.stderr.write(diff.stdout || "");
    process.stderr.write(diff.stderr || "");
    process.stderr.write("dist/ is out of date. Run `bun run build:runtime` and commit the result.\n");
    process.exit(1);
  }

  process.stdout.write("dist/ matches the rebuild\n");
}

main();
