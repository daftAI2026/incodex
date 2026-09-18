import { expect, test } from "bun:test";
import { ACCESSIBILITY_SETUP_COPY } from "../src/runtime/incognito-copy.ts";
import { sharedPermissionCopy } from "../src/permission-shared-copy.ts";

test("the CLI guide uses operation-neutral existing translations for every locale", () => {
  const original = JSON.stringify(ACCESSIBILITY_SETUP_COPY);
  const copy = sharedPermissionCopy(ACCESSIBILITY_SETUP_COPY);
  expect(Object.keys(copy)).toHaveLength(65);
  for (const [locale, source] of Object.entries(ACCESSIBILITY_SETUP_COPY)) {
    expect(copy[locale].body).toBe(source.addedTitle);
    expect(copy[locale].body.length).toBeGreaterThan(0);
    expect(copy[locale].errorBody).not.toContain("incodex install");
    expect(copy[locale].errorBody).toContain("incodex doctor");
    expect(copy[locale].dragInstruction).toBe(source.dragInstruction);
    expect(copy[locale].dragInstructionRuns).toBe(source.dragInstructionRuns);
  }
  expect(JSON.stringify(ACCESSIBILITY_SETUP_COPY)).toBe(original);
});
