import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { blobatarUri } from "blobatar/uri";

const inject = readFileSync(join(import.meta.dir, "inject.ts"), "utf8").replaceAll("\r\n", "\n");
const profileMask = readFileSync(
  join(import.meta.dir, "incognito-profile-mask.ts"),
  "utf8",
).replaceAll("\r\n", "\n");
const notice = readFileSync(join(import.meta.dir, "../../NOTICE"), "utf8");
const packageJson = JSON.parse(readFileSync(join(import.meta.dir, "../../package.json"), "utf8")) as {
  dependencies?: Record<string, string>;
};
const hatGlasses = readFileSync(join(import.meta.dir, "../../assets/hat-glasses.svg"), "utf8");
const circleX = readFileSync(join(import.meta.dir, "../../assets/circle-x.svg"), "utf8");

describe("hat-glasses stays after header remount", () => {
  test("does not disconnect the observer once the button exists", () => {
    expect(inject).not.toMatch(/if \(!needsInject\(\)\)[\s\S]{0,80}observer\.disconnect\(\)/);
    expect(inject).not.toMatch(/if \(uiReady\(\)\) return;/);
  });

  test("parks the button before the Search tooltip trigger boundary", () => {
    expect(inject).toContain("searchButtonPlacement(search)");
    expect(inject).toContain("placement.parent.insertBefore(btn, placement.before)");
    expect(inject).not.toContain("search.parentElement.insertBefore(btn, search)");
    expect(inject).not.toContain("cluster.insertBefore(btn, cluster.firstElementChild)");
  });

  test("watches documentElement so a replaced sidebar cluster is still seen", () => {
    expect(inject).toContain("document.documentElement");
    expect(inject).not.toContain("observer.observe(observeRoot()");
  });

  test("skips ensureButton while the hat is still beside the Search trigger", () => {
    expect(inject).toContain("buttonStillBesideSearch");
    expect(inject).toContain("if (!needsInject()) return");
  });
});

describe("incognito button exit affordance", () => {
  test("keeps both Lucide icons on the same 1.5px stroke", () => {
    const strokeWidth = (svg: string): string => svg.match(/stroke-width="([^"]+)"/)?.[1] ?? "";
    expect(strokeWidth(hatGlasses)).toBe("1.5");
    expect(strokeWidth(circleX)).toBe(strokeWidth(hatGlasses));
  });

  test("shows circle-x only while an incognito button is hovered", () => {
    expect(inject).toMatch(
      /isIncognitoWindow\(\)\s*&&\s*btn\.getAttribute\("data-incodex-hovered"\) === "true"\s*\? "circle-x"\s*:\s*"hat-glasses"/,
    );
  });

  test("routes pointer enter and leave through icon switching without changing click semantics", () => {
    expect(inject).toMatch(/function setButtonHover\(btn: HTMLElement, hovered: boolean\)[\s\S]*setButtonIcon\(btn\);/);
    expect(inject).toContain("setButtonHover(btn, true)");
    expect(inject).toContain("setButtonHover(btn, false)");

    const clickStart = inject.indexOf('btn.addEventListener(\n    "click"');
    const clickEnd = inject.indexOf("  );", clickStart);
    const clickHandler = inject.slice(clickStart, clickEnd);
    expect(clickHandler).toContain("event.preventDefault()");
    expect(clickHandler).toContain("event.stopImmediatePropagation()");
    expect(clickHandler).toContain("tooltipLifecycle.trigger()");
    expect(clickHandler).toContain("void activate().then((completed) => {");
    expect(clickHandler).toContain("if (completed && btn.isConnected) btn.blur()");
    expect(clickHandler).not.toContain("btn.focus()");
  });

  test("reports whether the requested action completed", () => {
    const activateStart = inject.indexOf("async function activate(): Promise<boolean>");
    const activateEnd = inject.indexOf("\nfunction ensureStyle", activateStart);
    const activate = inject.slice(activateStart, activateEnd);

    expect(activateStart).toBeGreaterThan(-1);
    expect(activate).toMatch(/if \(result\.ok\) \{[\s\S]*return true;/);
    expect(activate).toMatch(/showLaunchError\(\);\s*return false;/);
  });
});

describe("incognito banner placement", () => {
  test("uses the one official banner slot without a second mount model", () => {
    expect(inject).toContain("mountInOfficialBannerSlot(host)");
    expect(inject).toContain("const slot = findOfficialBannerSlot()");
    expect(inject).not.toContain("findLandingMount");
  });
});

describe("launch warning placement", () => {
  test("reuses the macOS home-banner constructor and a live official action on Windows", () => {
    expect(inject).toMatch(
      /function buildLanding\(\): HTMLElement \{[\s\S]*buildOfficialHomeBanner/,
    );
    expect(inject).toMatch(
      /function buildWindowsLaunchErrorBanner\(\): HTMLElement \{[\s\S]*buildOfficialHomeBanner/,
    );
    expect(inject).toContain("cloneOfficialPrimaryAction()");
    expect(inject).toMatch(
      /function showLaunchError\(\): void \{[\s\S]*buildWindowsLaunchErrorBanner\(\)[\s\S]*mountInOfficialBannerSlot/,
    );
    expect(inject).not.toContain("data-incodex-banner-mounted");
  });

  test("waits for the shared home-banner slot instead of falling back to a Windows overlay", () => {
    expect(inject).toMatch(
      /function showLaunchError\(\): void \{[\s\S]*if \(isWindowsRenderer\(\)\) \{[\s\S]*ensureLaunchError\(\);[\s\S]*return;/,
    );
    expect(inject).toMatch(
      /function needsInject\(\): boolean \{[\s\S]*launchErrorNeedsInject\(\)/,
    );
    expect(inject).toMatch(
      /function createMutationObserver\(\): MutationObserver \{[\s\S]*ensureLaunchError\(\)/,
    );
  });
});

describe("platform shortcut label", () => {
  test("keeps the macOS glyphs and labels the Windows control shortcut honestly", () => {
    expect(inject).toContain('isWindowsRenderer() ? "Ctrl+Shift+N" : SHORTCUT_LABEL');
    expect(inject).toContain("kbd.textContent = shortcutLabel()");
  });
});

describe("incognito profile mask", () => {
  test("pins one exact Blobatar release for offline generated avatars", () => {
    const blobatarVersion = packageJson.dependencies?.blobatar;
    expect(blobatarVersion).toMatch(/^\d+\.\d+\.\d+$/);
    expect(profileMask).toContain('from "blobatar/uri"');
    expect(profileMask).toContain("blobatarUri");
    expect(notice).toContain(`blobatar@${blobatarVersion}`);
    expect(notice).toContain("Copyright (c) 2026 Alain");
  });

  test("keeps the Blobatar Temporary golden output stable", () => {
    const digest = createHash("sha256").update(blobatarUri("Temporary")).digest("hex");
    expect(digest).toBe("05377318e542508086482ec3208f61e342e19a123cc1293e862959c19a493df0");
  });

  test("uses the CDP bootstrap value and only the unique sidebar profile footer", () => {
    expect(profileMask).toContain("__incodexProfileMask");
    expect(profileMask).toContain("findProfileFooter");
    expect(profileMask).toContain('button.sidebar-item[type="button"]');
    expect(profileMask).toContain(":scope > span.min-w-0.flex-1.truncate");
    expect(profileMask).toContain(":scope > img.rounded-full, :scope > span.rounded-full");
    expect(profileMask).toContain(":scope > [data-incodex-profile-mask-name]");
    expect(profileMask).toContain("candidates.length === 1");
    expect(inject).toContain("ensureProfileMask");
  });

  test("writes visual name and avatar values without taking over account semantics", () => {
    expect(profileMask).toContain("textContent = mask.name");
    expect(profileMask).toContain("avatar.src = mask.avatarDataUrl");
    expect(profileMask).toContain("identity.setAttribute(PROFILE_MASK_ATTR, \"true\")");
    expect(profileMask).not.toContain("profileFooter.setAttribute(\"aria-label\"");
    expect(profileMask).not.toContain("profileFooter.addEventListener(\"click\"");
  });

  test("masks the open account menu without changing its interaction semantics", () => {
    expect(profileMask).toContain("findProfileMenuIdentity");
    expect(profileMask).toContain('[role="menu"]');
    expect(profileMask).toContain('[role="menuitem"]');
    expect(profileMask).toContain(":scope > div > span.flex-1.min-w-0.truncate");
    expect(profileMask).toContain(":scope > div > span > img.icon-sm.rounded-full");
    expect(profileMask).toContain('profileFooter.getAttribute("aria-controls")');
    expect(profileMask).toMatch(
      /if \(!profileMenu\) return true;[\s\S]*if \(!menuIdentity\) return false;/,
    );
    expect(profileMask).toContain("ensureProfileMenuMask");
    expect(profileMask).not.toContain('setAttribute("role"');
    expect(profileMask).not.toContain('addEventListener("click"');
  });

  test("fills the native circular slot without distorting explicit images", () => {
    expect(profileMask).toContain('background: "circle"');
    expect(profileMask).toContain('avatar.style.objectFit = "cover"');
    expect(profileMask).toContain('avatar.style.objectPosition = "center"');
    expect(profileMask).toContain('avatar.style.backgroundSize = "cover"');
    expect(profileMask).toContain('avatar.style.backgroundPosition = "center"');
    expect(profileMask).toContain('avatar.style.objectPosition === "center center"');
    expect(profileMask).toContain('avatar.style.backgroundPosition === "center center"');
  });

  test("waits for avatar decoding before accepting the profile mask", () => {
    expect(profileMask).toContain("new Image()");
    expect(profileMask).toContain("probe: HTMLImageElement");
    expect(profileMask).toContain("state.probe = probe");
    expect(profileMask).toMatch(/addEventListener\(\s*"load"/);
    expect(profileMask).toMatch(/addEventListener\(\s*"error"/);
    expect(profileMask).toMatch(/\.status === "ready"/);
    expect(profileMask).toContain("profileAvatarDecoded(mask.avatarDataUrl)");
  });

  test("keeps the profile health surface fail-closed when the footer is ambiguous", () => {
    expect(profileMask).toContain("candidates.length === 1 ? candidates[0] : null");
    expect(profileMask).toContain("nameHost.textContent === mask.name");
    expect(profileMask).toContain("identityMaskHealth");
    expect(profileMask).toContain("profileMaskHealth");
    expect(inject).toContain(
      "window.__incodexRefreshProfileMaskHealth = profileMaskHealth",
    );
  });

  test("distinguishes a generated kind from a validated explicit data URL", () => {
    expect(profileMask).toContain('avatar.kind === "generated"');
    expect(profileMask).not.toContain("avatar.seed");
    expect(profileMask).toContain("avatar.dataUrl");
    expect(profileMask).toContain("blobatarUri(name,");
  });

  test("keeps ordinary observers on childList and opts into profile text attributes only when masked", () => {
    expect(inject).toContain("function observerOptions(): MutationObserverInit");
    expect(inject).toContain("childList: true");
    expect(inject).toContain("subtree: true");
    expect(inject).toContain("options.attributes = true");
    expect(inject).toContain("options.characterData = true");
    expect(inject).toContain("options.attributeFilter = PROFILE_OBSERVED_ATTRIBUTES");
    expect(inject).toMatch(/isIncognitoWindow\(\)\s*&&\s*window\.__incodexProfileMask !== null/);
    expect(inject).toContain("observer.observe(document.documentElement, observerOptions())");
  });

  test("rechecks masking when a staged profile menu is linked to its footer", () => {
    const observedAttributes = inject.match(
      /const PROFILE_OBSERVED_ATTRIBUTES = \[([\s\S]*?)\];/,
    )?.[1];

    expect(observedAttributes).toContain('"aria-controls"');
  });

  test("reobserves when CDP enables masking after Runtime startup", () => {
    expect(inject).toContain("__incodexMutationObserver");
    expect(inject).toContain("__incodexProfileObservationEnabled");
    expect(inject).toMatch(
      /if \(window\.__incodexStarted\) \{[\s\S]*ensureProfileMask\(\);[\s\S]*ensureMutationObserver\(\);/,
    );
    expect(inject).toMatch(
      /if \(!observer\) \{[\s\S]*window\.__incodexMutationObserver = observer;[\s\S]*\}[\s\S]*observer\.observe\(document\.documentElement, observerOptions\(\)\);/,
    );
    expect(inject).not.toContain(
      "if (observer && (!profileRequired || window.__incodexProfileObservationEnabled)) return;",
    );
    expect(inject).not.toContain("observer.disconnect()");
  });
});

describe("incodex tooltip lifecycle", () => {
  test("keeps a stable delay when the official provider cannot be discovered", () => {
    expect(inject).toContain("const TOOLTIP_FALLBACK_DELAY_MS = 700");
  });

  test("joins the discovered provider timing group without making it mandatory", () => {
    expect(inject).toContain("createOfficialTooltipTimingBridge(findSearchButton)");
    expect(inject).toContain("resolveDelay: providerTiming.resolveDelay");
    expect(inject).toContain("onOpen: providerTiming.activate");
    expect(inject).toContain("onClose: providerTiming.deactivate");
  });

  test("listens to the app-wide dismissal signal without dispatching the private event", () => {
    expect(inject).toContain(
      'window.addEventListener(TOOLTIP_DISMISS_EVENT, () => activeTooltipLifecycle?.dismiss())',
    );
    expect(inject).not.toContain("dispatchEvent(new Event(TOOLTIP_DISMISS_EVENT))");
  });

  test("does not override the official fit-content width utility", () => {
    expect(inject).not.toContain("width: max-content");
  });

  test("inherits the live official window zoom through a positioning host", () => {
    expect(inject).toContain("officialWindowZoom(document.documentElement)");
    expect(inject).toContain('tip.style.zoom = zoom === 1 ? "" : String(zoom)');
    expect(inject).toContain("TIP_HOST_ATTR");
  });

  test("clears an open tooltip without losing a connected button lifecycle", () => {
    expect(inject).toMatch(
      /function ensureButton\(\): void \{[\s\S]*if \(!search \|\| !placement\) \{[\s\S]*if \(btn\?\.isConnected\) dismissActiveTooltip\(\);[\s\S]*else disposeActiveTooltip\(\);[\s\S]*return;/,
    );
  });

  test("cancels pending and open tooltips on window blur and Escape", () => {
    expect(inject).toContain(
      'window.addEventListener("blur", () => activeTooltipLifecycle?.windowBlur())',
    );
    expect(inject).toContain(
      'window.addEventListener("focus", () => activeTooltipLifecycle?.windowFocus())',
    );
    expect(inject).toMatch(
      /function onKeydown\(event: KeyboardEvent\): void \{[\s\S]*event\.key === "Escape"[\s\S]*dismissActiveTooltip\(\);/,
    );
  });

  test("does not open while the official Search tooltip remains visible", () => {
    expect(inject).toMatch(
      /function injectedTooltipCanShow[\s\S]*!\(search && searchTooltipOpen\(search\)\)/,
    );
  });
});
