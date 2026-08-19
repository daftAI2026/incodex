import { describe, expect, test } from "bun:test";
import { lstatSync, readFileSync, readlinkSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "..");

function read(rel: string): string {
  return readFileSync(join(root, rel), "utf8");
}

describe("agent skills", () => {
  test("release-notes is a handwritten bilingual changelog skill", () => {
    const text = read(".claude/skills/release-notes/SKILL.md");
    expect(text).toContain("gh release edit");
    expect(text).toContain("### Changelog");
    expect(text).toContain("### 更新日志");
    expect(text).toContain("do not `gh release create`");
    expect(text).not.toContain("generate_release_notes: true");
    expect(text).toContain("post-reactions.sh");
  });

  test("post-reactions.sh posts the six GitHub reactions on a lowercase v tag", () => {
    const script = read(".claude/skills/release-notes/scripts/post-reactions.sh");
    expect(script).toContain("daftAI2026/incodex");
    expect(script).toContain("repos/${REPO}/releases");
    expect(script).toContain("+1 laugh hooray heart rocket eyes");
    expect(script).toContain('"$TAG" != v*');
    expect(script).not.toContain('"$TAG" != V*');
  });

  test("release-flow publishes by pushing a lowercase v tag", () => {
    const text = read(".claude/skills/release-flow/SKILL.md");
    expect(text).toContain("vX.Y.Z");
    expect(text).toContain("SHA256SUMS");
    expect(text).toContain("incodex-darwin-arm64");
    expect(text).toContain("package.json");
    expect(text).toContain("daftAI2026/homebrew-tap");
    expect(text).toContain("HOMEBREW_TAP_TOKEN");
    expect(text).toContain("Do not open a Homebrew/homebrew-core PR");
  });

  test("incodex CLI skill requires dry-run before destructive commands", () => {
    const text = read(".claude/skills/incodex/SKILL.md");
    expect(text).toContain("--dry-run");
    expect(text).toContain("--yes");
    expect(text).toContain("incodex open");
    expect(text).not.toContain("--confirm-live");
  });

  test("Codex skill paths are symlinks to .claude/skills", () => {
    for (const name of ["release-notes", "release-flow", "incodex", "bugs"]) {
      const link = join(root, ".agents/skills", name);
      expect(lstatSync(link).isSymbolicLink()).toBe(true);
      expect(readlinkSync(link)).toBe(`../../.claude/skills/${name}`);
    }
  });
});
