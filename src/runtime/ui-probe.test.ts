import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { deriveUiProbe } from "./incodex-ui-probe";

const inject = readFileSync(join(import.meta.dir, "inject.ts"), "utf8");

describe("minimal Runtime UI injection snapshot", () => {
  test("treats a banner as not applicable in an ordinary window", () => {
    expect(
      deriveUiProbe({
        incognito: false,
        buttonPresent: true,
        bannerPresent: false,
        bannerDismissed: false,
      }),
    ).toEqual({ button: "present", banner: "not-applicable", accepted: true });
  });

  test("distinguishes an incognito banner that is present, missing, or dismissed", () => {
    const base = { incognito: true, buttonPresent: true };

    expect(deriveUiProbe({ ...base, bannerPresent: true, bannerDismissed: false })).toEqual({
      button: "present",
      banner: "present",
      accepted: true,
    });
    expect(deriveUiProbe({ ...base, bannerPresent: false, bannerDismissed: false })).toEqual({
      button: "present",
      banner: "missing",
      accepted: false,
    });
    expect(deriveUiProbe({ ...base, bannerPresent: false, bannerDismissed: true })).toEqual({
      button: "present",
      banner: "dismissed",
      accepted: true,
    });
  });

  test("derives rejection from a missing button without growing a capability state machine", () => {
    expect(
      deriveUiProbe({
        incognito: true,
        buttonPresent: false,
        bannerPresent: true,
        bannerDismissed: false,
      }),
    ).toEqual({ button: "missing", banner: "present", accepted: false });
  });

  test("the injector retains the latest minimal snapshot for its caller", () => {
    expect(inject).toContain("window.__incodexUiProbe = deriveUiProbe");
    expect(inject).not.toMatch(/capabilit|appVersion|buildVersion/i);
  });

  test("refreshes a stale snapshot when did-finish-load reinjects the bundle", () => {
    expect(inject).toMatch(
      /if \(window\.__incodexStarted\) \{\s*refreshUiProbe\(\);\s*return;\s*\}/,
    );
  });
});
