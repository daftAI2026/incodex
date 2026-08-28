import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { deriveUiProbe } from "./incodex-ui-probe.ts";

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

  test("rejects a retained button when React removes the tooltip host", () => {
    const probe = deriveUiProbe({
      incognito: false,
      buttonPresent: true,
      bannerPresent: false,
      bannerDismissed: false,
      tooltipPresent: false,
    } as Parameters<typeof deriveUiProbe>[0]);

    expect({
      probeAccepted: probe.accepted,
      tracksMissingPortal: /function needsInject\(\): boolean \{[\s\S]*!tooltipMountStillPresent\(\)/.test(
        inject,
      ),
      restoresPortal: /function ensureButton\(\): void \{[\s\S]*if \(!btn\) btn = buildButton\(search\);[\s\S]*ensureTooltipMount\(\);/.test(
        inject,
      ),
      reportsPortal: inject.includes("tooltipPresent: tooltipMountStillPresent()"),
    }).toEqual({
      probeAccepted: false,
      tracksMissingPortal: true,
      restoresPortal: true,
      reportsPortal: true,
    });
  });

  test("repairs stale mounts when did-finish-load reinjects the bundle", () => {
    expect(inject).toMatch(
      /if \(window\.__incodexStarted\) \{[\s\S]*ensureButton\(\);[\s\S]*ensureLanding\(\);[\s\S]*ensureLaunchError\(\);[\s\S]*ensureProfileMask\(\);[\s\S]*refreshUiProbe\(\);[\s\S]*ensureMutationObserver\(\);[\s\S]*return;/,
    );
  });

  test("reobserves the document when a previous injector left an unobserved instance", () => {
    expect(inject).not.toMatch(
      /if \(observer && \(!profileRequired \|\| window\.__incodexProfileObservationEnabled\)\) return;/,
    );
    expect(inject).toMatch(
      /if \(!observer\) \{[\s\S]*window\.__incodexMutationObserver = observer;[\s\S]*\}[\s\S]*observer\.observe\(document\.documentElement, observerOptions\(\)\);/,
    );
  });
});
