import { describe, expect, test } from "bun:test";
import { confirmDecision, isTty, requireYesMessage } from "./confirm";

describe("confirmDecision", () => {
  test("clone and dry-run skip confirmation", () => {
    expect(confirmDecision({ clone: true, dryRun: false, yes: false, tty: false })).toBe("ok");
    expect(confirmDecision({ clone: false, dryRun: true, yes: false, tty: false })).toBe("ok");
  });

  test("--yes skips the prompt", () => {
    expect(confirmDecision({ clone: false, dryRun: false, yes: true, tty: true })).toBe("ok");
    expect(confirmDecision({ clone: false, dryRun: false, yes: true, tty: false })).toBe("ok");
  });

  test("TTY without --yes asks once", () => {
    expect(confirmDecision({ clone: false, dryRun: false, yes: false, tty: true })).toBe("ask");
  });

  test("non-TTY without --yes requires --yes", () => {
    expect(confirmDecision({ clone: false, dryRun: false, yes: false, tty: false })).toBe("require-yes");
    expect(requireYesMessage("install")).toContain("incodex install --yes");
  });

  test("isTty needs both stdin and stdout", () => {
    expect(isTty({ isTTY: true }, { isTTY: true })).toBe(true);
    expect(isTty({ isTTY: true }, { isTTY: false })).toBe(false);
    expect(isTty({}, { isTTY: true })).toBe(false);
  });
});
