import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const dependabot = readFileSync(join(root, ".github/dependabot.yml"), "utf8");

type UpdateEntry = {
  ecosystem: string;
  source: string;
};

function updateEntries(source: string): UpdateEntry[] {
  const starts = [...source.matchAll(/^ {2}- package-ecosystem:\s*"([^"]+)"\s*$/gm)];
  return starts.map((match, index) => ({
    ecosystem: match[1],
    source: source.slice(match.index, starts[index + 1]?.index ?? source.length),
  }));
}

function scalar(entry: UpdateEntry, name: string): string | null {
  return (
    entry.source
      .match(new RegExp(`^    ${name}:\\s*(?:"([^"]+)"|(\\d+))\\s*$`, "m"))
      ?.slice(1)
      .find(Boolean) ?? null
  );
}

function groupsBlock(entry: UpdateEntry): string {
  const start = entry.source.indexOf("\n    groups:");
  if (start < 0) return "";
  const tail = entry.source.slice(start + "\n    groups:".length);
  const end = tail.search(/^ {4}[a-z0-9-]+:\s*$/m);
  return end < 0 ? tail : tail.slice(0, end);
}

function groupNames(entry: UpdateEntry): string[] {
  return [...groupsBlock(entry).matchAll(/^ {6}([a-z0-9-]+):\s*$/gm)].map(
    (match) => match[1],
  );
}

function groupPatterns(entry: UpdateEntry, group: string): string[] {
  const groups = groupsBlock(entry);
  const start = groups.indexOf(`      ${group}:`);
  if (start < 0) return [];
  const tail = groups.slice(start + `      ${group}:`.length);
  const nextGroup = tail.search(/^ {6}[a-z0-9-]+:\s*$/m);
  const block = nextGroup < 0 ? tail : tail.slice(0, nextGroup);
  return [...block.matchAll(/^ {10}- "([^"]+)"\s*$/gm)].map((match) => match[1]);
}

describe("Dependabot update coverage", () => {
  test("covers Actions, Cargo, and Bun from the repository root every week", () => {
    const entries = updateEntries(dependabot);
    expect(entries.map((entry) => entry.ecosystem).sort()).toEqual([
      "bun",
      "cargo",
      "github-actions",
    ]);

    for (const entry of entries) {
      expect(scalar(entry, "directory")).toBe("/");
      expect(entry.source).toMatch(/^ {4}schedule:\s*\n {6}interval:\s*"weekly"\s*$/m);
    }
  });

  test("groups each ecosystem instead of opening noisy one-package PRs", () => {
    const entries = new Map(updateEntries(dependabot).map((entry) => [entry.ecosystem, entry]));

    for (const ecosystem of ["cargo", "bun"] as const) {
      const entry = entries.get(ecosystem);
      expect(entry).toBeDefined();
      if (!entry) continue;
      expect(scalar(entry, "open-pull-requests-limit")).toBe("3");
      expect(groupNames(entry)).toEqual([ecosystem]);
      expect(groupPatterns(entry, ecosystem)).toEqual(["*"]);
    }

    const actions = entries.get("github-actions");
    expect(actions).toBeDefined();
    if (actions) {
      expect(groupNames(actions)).toEqual(["github-actions"]);
      expect(groupPatterns(actions, "github-actions")).toEqual(["*"]);
    }
  });

  test("does not invent npm or cross-ecosystem update policy", () => {
    const ecosystems = updateEntries(dependabot).map((entry) => entry.ecosystem);
    expect(ecosystems).not.toContain("npm");
    expect(dependabot).not.toMatch(/^multi-ecosystem-groups:/m);
  });
});
