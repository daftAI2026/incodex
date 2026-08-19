import { readFileSync } from "node:fs";
import { join } from "node:path";

export function cliVersion(): string {
  const pkg = JSON.parse(readFileSync(join(import.meta.dir, "..", "package.json"), "utf8")) as {
    version?: string;
  };
  return pkg.version || "0.0.0";
}
