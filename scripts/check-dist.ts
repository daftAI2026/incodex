import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

export function runtimeCheckEnvironment(_committedSourceCommit: string, environment: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  return { ...environment };
}

function main(): void {
  const built = spawnSync("bun", ["src/build-runtime.ts"], {
    cwd: root,
    encoding: "utf8",
    stdio: "inherit",
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

if (import.meta.main) main();
