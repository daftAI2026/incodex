import { mkdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";

const outDir = process.argv[2] ?? "release-cli";
mkdirSync(outDir, { recursive: true });

const targets = [
  { target: "bun-darwin-arm64", name: "incodex-darwin-arm64" },
  { target: "bun-darwin-x64", name: "incodex-darwin-x64" },
] as const;

for (const { target, name } of targets) {
  const outfile = join(outDir, name);
  const built = spawnSync(
    "bun",
    ["build", "src/cli.ts", "--compile", `--target=${target}`, `--outfile=${outfile}`],
    { stdio: "inherit" },
  );
  if (built.status !== 0) process.exit(built.status ?? 1);
  const signed = spawnSync("codesign", ["--sign", "-", "--force", outfile], { encoding: "utf8" });
  if (signed.status !== 0) {
    console.error(signed.stderr || `codesign failed for ${outfile}`);
    process.exit(signed.status ?? 1);
  }
}
