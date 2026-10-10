import { describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import vm from "node:vm";
import { blobatarUri } from "blobatar/uri";
import { findProfileMenuIdentity, profileMaskHealth, refreshProfileMaskHealth } from "./incognito-profile-mask.ts";

type ProfileNavigationSurface = boolean | "loading" | "wrapped-loading" | "loading-text" | "competing-loading";

function withProfileNavigation(
  run: (navigate: (count: number, settings?: ProfileNavigationSurface, recognized?: boolean, trigger?: "menu" | "controls") => void) => void,
) {
  const globals = ["window", "document", "HTMLImageElement"];
  const previous = globals.map((key) => Object.getOwnPropertyDescriptor(globalThis, key));
  class Avatar {
    src = "official-avatar";
    attrs = new Map<string, string>();
    style = {
      objectFit: "",
      get objectPosition() { return "center center"; },
      set objectPosition(_value: string) {},
    };
    setAttribute(key: string, value: string) { this.attrs.set(key, value); }
    getAttribute(key: string) { return key === "src" ? this.src : this.attrs.get(key) ?? null; }
  }
  const footer = (recognized = true, trigger: "menu" | "controls" = "menu") => {
    const attrs = new Map<string, string>([trigger === "menu" ? ["aria-haspopup", "menu"] : ["aria-controls", "account-menu"]]);
    const name = {
      textContent: "Official Name",
      attrs: new Map<string, string>(),
      setAttribute(key: string, value: string) { this.attrs.set(key, value); },
      getAttribute(key: string) { return this.attrs.get(key) ?? null; },
    };
    const avatar = new Avatar();
    return {
      setAttribute: (key: string, value: string) => attrs.set(key, value),
      getAttribute: (key: string) => attrs.get(key) ?? null,
      querySelector: (selector: string) => {
        if (!recognized) return null;
        if (selector.includes("span.min-w-0.flex-1.truncate")) return name;
        if (selector.includes("img.rounded-full")) return avatar;
        return null;
      },
    };
  };
  let footers = [footer()];
  let inSettings: ProfileNavigationSurface = false;
  const settingsNavigation = {
    querySelector: (selector: string) =>
      ['input[role="searchbox"]', 'button.sidebar-item[role="link"]'].includes(selector) ? {} : null,
  };
  const loadingNavigation = {
    get textContent() { return inSettings === "loading-text" ? "Unexpected identity" : "加载中"; },
    get childNodes() { return inSettings === "loading-text" ? [{}, {}] : [{}]; },
    firstElementChild: { classList: { contains: (name: string) => name === "invisible" } },
    querySelector: (selector: string) => selector === ":scope > .invisible" ? {} : null,
  };
  const replacements = [
    {
      __incodexIncognito: true,
      __incodexProfileMask: { name: "Temporary", avatar: { kind: "generated" } },
      __incodexProfileAvatarDecodeState: {
        dataUrl: blobatarUri("Temporary", { background: "circle" }), status: "ready", probe: null,
      },
    },
    {
      querySelectorAll: (selector: string) => {
        if (selector === "nav.sidebar-navigation") {
          return inSettings === true || inSettings === "competing-loading" ? [settingsNavigation] : [];
        }
        if (selector === 'button.sidebar-item[type="button"]') return footers;
        if (typeof inSettings !== "string") return [];
        const skeleton = '<nav aria-busy="true"><div class="invisible">加载中</div></nav>';
        const content = inSettings === "wrapped-loading" ? `<div><div>${skeleton}</div></div>` : skeleton;
        const matches: typeof loadingNavigation[] = [];
        // 使用真实 CSS selector 解析器，避免 mock 把错误的父子关系也判成命中。
        new HTMLRewriter().on(selector, { element() { matches.push(loadingNavigation); } })
          .transform(`<aside class="app-shell-left-panel">${content}</aside>`);
        return matches;
      },
      getElementById: () => null,
    },
    Avatar,
  ];
  try {
    globals.forEach((key, index) => {
      Object.defineProperty(globalThis, key, {
        configurable: true, writable: true, value: replacements[index],
      });
    });
    run((count, settings = false, recognized = true, trigger = "menu") => {
      inSettings = settings;
      footers = Array.from({ length: count }, () => footer(recognized, trigger));
    });
  } finally {
    globals.forEach((key, index) => {
      if (previous[index]) Object.defineProperty(globalThis, key, previous[index]!);
      else Reflect.deleteProperty(globalThis, key);
    });
  }
}

describe("profile mask navigation scope", () => {
  test("allows settings without a footer and repairs a newly mounted footer on return", () => {
    withProfileNavigation((navigate) => {
      expect(profileMaskHealth()).toBe(false);
      expect(refreshProfileMaskHealth()).toBe(true);
      navigate(0, true);
      expect(profileMaskHealth()).toBe(true);
      expect(refreshProfileMaskHealth()).toBe(true);
      navigate(1);
      expect(profileMaskHealth()).toBe(false);
      expect(refreshProfileMaskHealth()).toBe(true);
    });
  });

  test("accepts the empty busy settings skeleton before navigation mounts", () => {
    withProfileNavigation((navigate) => {
      expect(refreshProfileMaskHealth()).toBe(true);
      navigate(0, "loading");
      expect(refreshProfileMaskHealth()).toBe(true);
      navigate(0, true);
      expect(refreshProfileMaskHealth()).toBe(true);
      navigate(1);
      expect(refreshProfileMaskHealth()).toBe(true);
    });
  });

  test("rejects busy navigation with unexpected text or a surviving account trigger", () => {
    withProfileNavigation((navigate) => {
      navigate(0, "loading-text");
      expect(refreshProfileMaskHealth()).toBe(false);
      navigate(1, "loading", false);
      expect(refreshProfileMaskHealth()).toBe(false);
    });
  });

  test("accepts the Windows settings skeleton inside sidebar layout wrappers", () => {
    // Windows Store 26.901.6511.0: aside.app-shell-left-panel > div > div > nav.
    // HTMLRewriter 解析真实 selector；完整 UI 生命周期另在 Store App 中验证。
    withProfileNavigation((navigate) => {
      expect(refreshProfileMaskHealth()).toBe(true);
      navigate(0, "wrapped-loading");
      expect(refreshProfileMaskHealth()).toBe(true);
      navigate(0, true);
      expect(refreshProfileMaskHealth()).toBe(true);
      navigate(1);
      expect(profileMaskHealth()).toBe(false);
      expect(refreshProfileMaskHealth()).toBe(true);
    });
  });

  test("rejects a settings surface with a surviving aria-controls identity", () => {
    withProfileNavigation((navigate) => {
      navigate(1, true, false, "controls");
      expect(refreshProfileMaskHealth()).toBe(false);
      navigate(1, "loading", false, "controls");
      expect(refreshProfileMaskHealth()).toBe(false);
    });
  });

  test("rejects competing ready and loading navigation surfaces", () => {
    withProfileNavigation((navigate) => {
      navigate(0, "competing-loading");
      expect(refreshProfileMaskHealth()).toBe(false);
    });
  });

  test("does not accept an absent identity without a verified settings surface", () => {
    withProfileNavigation((navigate) => {
      navigate(0);
      expect(profileMaskHealth()).toBe(false);
      expect(refreshProfileMaskHealth()).toBe(false);
    });
  });

  test.each([false, true])("rejects an unrecognized account trigger even with settings=%s", (settings) => {
    withProfileNavigation((navigate) => {
      navigate(1, settings, false);
      expect(profileMaskHealth()).toBe(false);
      expect(refreshProfileMaskHealth()).toBe(false);
    });
  });

  test("does not confuse an ambiguous visible identity with an absent surface", () => {
    withProfileNavigation((navigate) => {
      navigate(2);
      expect(profileMaskHealth()).toBe(false);
      expect(refreshProfileMaskHealth()).toBe(false);
    });
  });
});

// Windows 26.901.6511.0 实测：姓名增加 div，头像增加 span。
// 只模拟 DOM 查询边界；选择器含义另由真实 Store App 回归验证。
function profileMenuFixture(layout: "legacy" | "nested", avatar = true) {
  const namePath = layout === "legacy"
    ? ":scope > div > span.flex-1.min-w-0.truncate"
    : ":scope > div > div.flex-1.min-w-0 > span.min-w-0.truncate";
  const avatarPath = layout === "legacy"
    ? ":scope > div > span > img.icon-sm.rounded-full"
    : ":scope > div > span > span > img.icon-sm.rounded-full";
  const name = {};
  const image = {};
  return {
    querySelector(selector: string) {
      const paths = selector.split(",").map((path) => path.trim());
      if (paths.includes(namePath)) return name;
      if (avatar && paths.includes(avatarPath)) return image;
      return null;
    },
  } as unknown as HTMLElement;
}

function profileMenuWith(items: HTMLElement[]): HTMLElement {
  return {
    querySelectorAll: (selector: string) => selector === '[role="menuitem"]' ? items : [],
  } as unknown as HTMLElement;
}

describe("live profile menu structure regression", () => {
  test.each(["legacy", "nested"] as const)("recognizes the unique %s profile identity", (layout) => {
    const identity = profileMenuFixture(layout);
    expect(findProfileMenuIdentity(profileMenuWith([identity]))).toBe(identity);
  });

  test("does not treat a name-only menu action as an identity", () => {
    expect(findProfileMenuIdentity(profileMenuWith([profileMenuFixture("nested", false)]))).toBeNull();
  });

  test("rejects simultaneous old and new identity candidates", () => {
    expect(findProfileMenuIdentity(profileMenuWith([
      profileMenuFixture("legacy"), profileMenuFixture("nested"),
    ]))).toBeNull();
  });
});

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
  test("keeps the original line hat and lets host currentColor control opacity", () => {
    const rootAttributes = hatGlasses.match(/<svg\b([^>]*)>/)?.[1] ?? "";
    const strokeWidth = (svg: string): string => svg.match(/stroke-width="([^"]+)"/)?.[1] ?? "";
    expect(rootAttributes).toContain('fill="none"');
    expect(rootAttributes).toContain('stroke="currentColor"');
    expect(rootAttributes).toContain('stroke-width="1.5"');
    // Semi-transparent currentColor must be painted once, including crossings.
    expect(hatGlasses.match(/<(?:path|circle)\b/g)).toHaveLength(1);
    expect(hatGlasses).toContain('M14 18a2 2 0 0 0-4 0');
    expect(hatGlasses).toContain('M19 11l-2.11-6.657');
    expect(hatGlasses).toContain('M2 11h20');
    expect(hatGlasses).toContain('M20 18a3 3 0 1 1-6 0a3 3 0 1 1 6 0Z');
    expect(hatGlasses).toContain('M10 18a3 3 0 1 1-6 0a3 3 0 1 1 6 0Z');
    expect(rootAttributes).not.toMatch(/\bopacity=/);
    expect(hatGlasses).not.toMatch(/<(?:path|circle)\b[^>]*(?:opacity|stroke-opacity)=/);
    expect(strokeWidth(circleX)).toBe("1.5");
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

  test("reports whether the requested action completed", async () => {
    const activateStart = inject.indexOf("async function activate(): Promise<boolean>");
    const activateEnd = inject.indexOf("\nfunction ensureStyle", activateStart);
    const activate = inject.slice(activateStart, activateEnd);

    expect(activateStart).toBeGreaterThan(-1);
    const js = new Bun.Transpiler({ loader: "ts" }).transformSync(activate);
    for (const ok of [true, false]) {
      const outcome = await vm.runInNewContext(`${js}; activate()`, {
        dismissActiveTooltip() {}, isIncognitoWindow: () => false,
        beginRendererAction: () => 1, requestAction: async () => ({ ok }),
        settleRendererAction() {}, window: {},
      });
      expect(outcome).toBe(ok);
    }
  });
});

describe("incognito banner placement", () => {
  function bannerCandidate(className: string, injected = false, homeOwned = false) {
    return {
      parentElement: { matches: (selector: string) => homeOwned && selector === '[data-codex-composer-root][data-composer-placement="home"]' },
      getAttribute: (name: string) => name === "class" ? className : null,
      hasAttribute: (name: string) => injected && name === "data-incodex-banner-host",
    };
  }

  function discoverBannerSlot(candidates: ReturnType<typeof bannerCandidate>[]) {
    const start = inject.indexOf("function classNameOf(");
    const end = inject.indexOf("function ensureLaunchError(", start);
    expect(start).toBeGreaterThan(-1);
    expect(end).toBeGreaterThan(start);
    const js = new Bun.Transpiler({ loader: "ts" }).transformSync(inject.slice(start, end));
    return vm.runInNewContext(`${js}; findOfficialBannerSlot()`, {
      document: { querySelectorAll: (selector: string) => selector === "div" ? candidates : [] },
      BANNER_HOST_ATTR: "data-incodex-banner-host",
    });
  }

  test("finds the new official home banner slot, not the composer banner-aware wrapper", () => {
    const slot = bannerCandidate("not-has-[>:not([hidden])]:hidden electron:mx-[var(--home-composer-inline-inset)] electron:has-[[data-home-beacon-banner]]:mx-0");
    const composer = bannerCandidate("px-[var(--home-composer-inline-inset)] pb-2 empty:hidden has-[[data-home-beacon-banner]]:px-0");
    expect(discoverBannerSlot([slot, composer])).toBe(slot);
  });

  test("preserves the legacy slot and rejects competing official slots", () => {
    const legacy = bannerCandidate("home-banners");
    const current = bannerCandidate("not-has-[>:not([hidden])]:hidden electron:has-[[data-home-beacon-banner]]:mx-0");
    expect(discoverBannerSlot([legacy])).toBe(legacy);
    expect(discoverBannerSlot([legacy, current])).toBeNull();
    expect(discoverBannerSlot([current, current])).toBeNull();
  });

  test("uses the home-owned composer banner slot when the separate home slot is absent", () => {
    const classes = "px-[var(--home-composer-inline-inset)] pb-2 empty:hidden has-[[data-home-beacon-banner]]:px-0";
    const home = bannerCandidate(classes, false, true);
    const conversation = bannerCandidate(classes);
    const separate = bannerCandidate("home-banners");
    expect(discoverBannerSlot([home, conversation])).toBe(home);
    expect(discoverBannerSlot([conversation])).toBeNull();
    expect(discoverBannerSlot([separate, home])).toBe(separate);
    expect(discoverBannerSlot([home, bannerCandidate(classes, false, true)])).toBeNull();
  });

  test("uses the official notification renderer for privacy and errors", () => {
    expect(inject).toContain("notifications.ensure(slot, copy)");
    expect(inject).toContain("notifications.showError({");
    expect(inject).not.toContain("buildOfficialHomeBanner");
    expect(inject).not.toContain("data-incodex-launch-error-overlay");
    expect(inject).not.toContain("cloneOfficialPrimaryAction");
  });
});

describe("notification reconciliation in background windows", () => {
  test("reconciles native notifications before a suspended animation frame", () => {
    const start = inject.indexOf("function createMutationObserver(): MutationObserver");
    const end = inject.indexOf("function ensureMutationObserver()", start);
    const js = new Bun.Transpiler({ loader: "ts" }).transformSync(inject.slice(start, end));
    let reconcile = 0; let probe = 0; let frames = 0;
    const observer = vm.runInNewContext(`${js}; createMutationObserver()`, {
      observe: (callback: () => void) => ({ callback }),
      active: true, epoch: 1, ownedFrames: new Set(),
      ensureLanding: () => { reconcile += 1; },
      refreshUiProbe: () => { probe += 1; },
      requestAnimationFrame: () => { frames += 1; },
    });
    observer.callback(); observer.callback();
    expect(reconcile).toBe(2);
    expect(probe).toBe(2);
    expect(frames).toBe(1);
  });
});

describe("platform shortcut label", () => {
  test("keeps the macOS glyphs and labels the Windows control shortcut honestly", () => {
    expect(inject).toContain('isWindowsRenderer() ? "Ctrl+Shift+N" : SHORTCUT_LABEL');
    expect(inject).toContain("kbd.textContent = shortcutLabel()");
  });
});

describe("macOS native menu copy", () => {
  test("sends the existing localized copy to both native menus", () => {
    expect(inject).toContain('action: "configure-dock-menu"');
    expect(inject).toContain('action: "configure-status-menu"');
    expect(inject).toContain('t(isIncognitoWindow() ? "title" : "open")');
    expect(inject).toContain('window.__incodexPlatform !== "darwin"');
    expect(inject).toContain("configureDockMenu()");
    expect(inject).toContain("configureStatusMenu()");
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
      /if \(!profileMenu\) return profileFooter.getAttribute\("aria-expanded"\) !== "true";[\s\S]*if \(!menuIdentity\) return false;/,
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
      "window.__incodexRefreshProfileMaskHealth = refreshProfileMaskHealth",
    );
  });

  test("repairs the profile synchronously before native health polling without waiting for a frame", () => {
    expect(profileMask).toMatch(
      /export function refreshProfileMaskHealth\(\): boolean \{\s*ensureProfileMask\(\);\s*return profileMaskHealth\(\);\s*\}/,
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
    // Repeated-evaluation behavior is exercised against the real bundle in
    // renderer-generation.test.ts, including late native CDP masking.
    expect(inject).toMatch(
      /if \(!observer\) \{[\s\S]*window\.__incodexMutationObserver = observer;[\s\S]*\}[\s\S]*observer\.observe\(document\.documentElement, observerOptions\(\)\);/,
    );
    expect(inject).not.toContain(
      "if (observer && (!profileRequired || window.__incodexProfileObservationEnabled)) return;",
    );
  });
});

describe("incodex tooltip lifecycle", () => {
  for (const incognito of [false, true]) {
    test(`never uses native title before sampling or after remount (incognito=${incognito})`, () => {
      const attrs = new Map<string, string>([["aria-label", incognito ? "Exit incognito" : "Open incognito"]]);
      const button = {
        getAttribute: (key: string) => attrs.get(key) ?? null,
        setAttribute: (key: string, value: string) => attrs.set(key, value),
        removeAttribute: (key: string) => attrs.delete(key),
      };
      const kbd = { className: "" };
      const tip = { className: "", querySelector: () => kbd };
      let sample: { className: string; shortcutClassName: string } | null = null;
      let hidden = false;
      const source = inject.slice(inject.indexOf("function syncTooltipPresentation():"), inject.indexOf("function tooltipEl():"));
      const js = new Bun.Transpiler({ loader: "ts" }).transformSync(source);
      const context = vm.createContext({
        findSearchButton: () => ({}), observeOfficialTooltip: () => {},
        officialTooltipPresentation: { read: () => sample },
        document: { querySelector: (selector: string) => selector === "[button]" ? button : tip },
        BTN_ATTR: "button", TIP_ATTR: "tip", hideTooltip: () => { hidden = true; },
        tooltipState: { renderer: null },
        labelFor: () => "Incognito", isIncognitoWindow: () => incognito, shortcutLabel: () => "Shift+Cmd+N",
      });
      const sync = new vm.Script(`${js}; syncTooltipPresentation`).runInContext(context) as () => boolean;
      expect(sync()).toBe(false);
      expect(hidden).toBe(true);
      expect(attrs.has("title")).toBe(false);
      expect(attrs.get("aria-label")).toBe(incognito ? "Exit incognito" : "Open incognito");

      sample = { className: "official-tooltip", shortcutClassName: "official-shortcut" };
      expect(sync()).toBe(true);
      expect(tip.className).toBe("official-tooltip");
      expect(kbd.className).toBe("official-shortcut");
      expect(attrs.has("title")).toBe(false);

      // A remounted Search loses its cached sample; also clear titles from an older injector.
      sample = null;
      attrs.set("title", "stale native fallback");
      expect(sync()).toBe(false);
      expect(attrs.has("title")).toBe(false);
    });
  }

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
    // Stable event forwarding is verified by the actual injector lifecycle suite.
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
    // Stable blur/focus forwarding is verified by the actual injector suite.
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
