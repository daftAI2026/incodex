import { describe, expect, test } from "bun:test";
import { commandHelp, rootHelp } from "./help";

describe("help text", () => {
  test("root help lists commands and the menu, not bun src/cli.ts", () => {
    const text = rootHelp();
    expect(text).toContain("incodex <command>");
    expect(text).toContain("install");
    expect(text).toContain("inc is the same program");
    expect(text).not.toContain("bun src/cli.ts");
    expect(text).not.toContain("--confirm-live");
    expect(text).not.toContain("--live");
  });

  test("install help shows short examples", () => {
    const text = commandHelp("install");
    expect(text).toContain("incodex install");
    expect(text).toContain("incodex install --yes");
    expect(text).toContain("--dry-run");
    expect(text).not.toContain("bun src/cli.ts");
    expect(text).not.toContain("--confirm-live");
  });
});
