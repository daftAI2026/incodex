import { readFileSync } from "node:fs";

export function shardFiles(command: string, shard: number, count: number): string[] {
  const [runtime, subcommand, ...files] = command.trim().split(/\s+/);
  if (runtime !== "bun" || subcommand !== "test" || files.length === 0 ||
      !files.every(file => /^[\w./-]+\.test\.ts$/.test(file)) ||
      !Number.isInteger(count) || count < 1 || count > files.length ||
      !Number.isInteger(shard) || shard < 0 || shard >= count) {
    throw new Error("Invalid test command or shard; refusing incomplete coverage");
  }
  return files.filter((_, index) => index % count === shard);
}

if (import.meta.main) {
  const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
  const files = shardFiles(pkg.scripts["test:macos"], Number(process.argv[2]), Number(process.argv[3]));
  console.log(`Running ${files.length} macOS test files`);
  const result = Bun.spawnSync([process.execPath, "test", ...files], {
    stdin: "inherit", stdout: "inherit", stderr: "inherit",
  });
  process.exit(result.exitCode);
}
