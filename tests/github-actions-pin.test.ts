import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const workflows = join(import.meta.dir, "..", ".github/workflows");
const pinned = /^\s+-?\s*uses:\s+\S+@([0-9a-f]{40})\s+#\s+\S+/;

describe("GitHub Actions pins", () => {
  test("every third-party action is pinned to a commit SHA with a version comment", () => {
    const files = readdirSync(workflows).filter((name) => name.endsWith(".yml") || name.endsWith(".yaml"));
    expect(files.length).toBeGreaterThan(0);
    const uses: string[] = [];
    for (const name of files) {
      const text = readFileSync(join(workflows, name), "utf8");
      for (const line of text.split("\n")) {
        if (!line.includes("uses:")) continue;
        uses.push(`${name}: ${line.trim()}`);
        expect(line).toMatch(pinned);
      }
    }
    expect(uses.length).toBeGreaterThan(0);
  });
});
