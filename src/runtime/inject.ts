import { STRIP_CLONE_ATTRS } from "./compatibility/default-adapter";
import { isSearchLabel } from "./compatibility/search-labels";
import { type CopyKey, translate, resolveLocale as matchLocale } from "./incognito-copy";
import { iconFor, type IncognitoButtonIcon } from "./incognito-icon";
import { deriveUiProbe } from "./incodex-ui-probe";
import { searchButtonPlacement, searchTooltipOpen } from "./search-button-placement";
import { createTooltipLifecycle, type TooltipLifecycle } from "./tooltip-lifecycle";

const STYLE_ID = "incodex-privacy-style";
const BTN_ATTR = "data-incodex-privacy-toggle";
const TIP_ATTR = "data-incodex-tooltip";
const LANDING_ATTR = "data-incodex-landing";
const ERROR_ATTR = "data-incodex-launch-error";
const SHORTCUT_LABEL = "⇧⌘N";
const TOOLTIP_FALLBACK_DELAY_MS = 700;
const TOOLTIP_DISMISS_EVENT = "codex:dismiss-tooltips";

let activeTooltipLifecycle: TooltipLifecycle | null = null;

function dismissActiveTooltip(): void {
  activeTooltipLifecycle?.dismiss();
}

function disposeActiveTooltip(): void {
  activeTooltipLifecycle?.dispose();
  activeTooltipLifecycle = null;
}

const ICON_SVG = `{{HAT_GLASSES_SVG}}`;
const EXIT_ICON_SVG = `{{CIRCLE_X_SVG}}`;
function isIncognitoWindow(): boolean {
  if (typeof window.__incodexIncognito === "boolean") return window.__incodexIncognito;
  return false;
}

function currentLocale(): string {
  return matchLocale(window.__incodexLocale || document.documentElement.lang || navigator.language || "en");
}

function t(key: CopyKey): string {
  return translate(currentLocale(), key);
}

function labelFor(on: boolean): string {
  return on ? t("exit") : t("open");
}

function buttonIcon(btn: HTMLElement): IncognitoButtonIcon {
  return iconFor({
    incognito: isIncognitoWindow(),
    hovered: btn.getAttribute("data-incodex-hovered") === "true",
  });
}

function createButtonIcon(source: string, name: IncognitoButtonIcon, sample: SVGElement | null): SVGElement | null {
  const wrap = document.createElement("span");
  wrap.innerHTML = source.trim();
  const svg = wrap.firstElementChild as SVGElement | null;
  if (!svg) return null;
  svg.setAttribute("data-incodex-icon", name);
  svg.setAttribute("class", sample?.getAttribute("class") || "icon-xs");
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("width", sample?.getAttribute("width") || "16");
  svg.setAttribute("height", sample?.getAttribute("height") || "16");
  return svg;
}

function setButtonIcon(btn: HTMLElement): void {
  const name = buttonIcon(btn);
  const current = btn.querySelector<SVGElement>("svg[data-incodex-icon]");
  if (current?.getAttribute("data-incodex-icon") === name) return;
  const source = name === "circle-x" ? EXIT_ICON_SVG : ICON_SVG;
  const sample = current || btn.querySelector<SVGElement>("svg");
  const next = createButtonIcon(source, name, sample);
  if (!next) return;
  if (current) current.replaceWith(next);
  else if (sample) sample.replaceWith(next);
  else btn.append(next);
}

function setButtonHover(btn: HTMLElement, hovered: boolean): void {
  btn.setAttribute("data-incodex-hovered", hovered ? "true" : "false");
  setButtonIcon(btn);
}

function apply(): void {
  const incognito = isIncognitoWindow();
  document.documentElement.setAttribute("data-incodex-window", incognito ? "incognito" : "normal");
  const btn = document.querySelector<HTMLElement>(`[${BTN_ATTR}]`);
  if (btn) {
    btn.setAttribute("aria-pressed", incognito ? "true" : "false");
    btn.setAttribute("aria-label", labelFor(incognito));
    setButtonIcon(btn);
  }
  const label = document.querySelector<HTMLElement>("[data-incodex-tooltip-label]");
  if (label) label.textContent = labelFor(incognito);
}

function newRequestId(): string {
  return `incodex-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

async function requestAction(action: "open" | "quit"): Promise<{ ok: boolean; reason?: string; code?: string }> {
  if (!window.incodex?.requestIncognitoAction) {
    return { ok: false, reason: "unavailable", code: "UNAVAILABLE" };
  }
  try {
    return (
      (await window.incodex.requestIncognitoAction({ action, requestId: newRequestId() })) ?? {
        ok: false,
        reason: "unavailable",
        code: "UNAVAILABLE",
      }
    );
  } catch {
    return { ok: false, reason: "ipc-failed", code: "IPC_FAILED" };
  }
}

async function activate(): Promise<void> {
  dismissActiveTooltip();
  if (isIncognitoWindow()) {
    const result = await requestAction("quit");
    if (!result.ok) window.close();
    return;
  }
  const result = await requestAction("open");
  if (result.ok) {
    hideLaunchError();
    return;
  }
  showLaunchError();
}

function ensureStyle(): void {
  let style = document.getElementById(STYLE_ID) as HTMLStyleElement | null;
  if (!style) {
    style = document.createElement("style");
    style.id = STYLE_ID;
    document.head.append(style);
  }
  style.textContent = `
    [${TIP_ATTR}] {
      position: fixed;
      z-index: 50;
      display: none;
      max-width: min(20rem, calc(100vw - 16px));
      pointer-events: none !important;
      user-select: none;
      box-sizing: border-box;
    }
    [${TIP_ATTR}][data-open="true"] { display: block; }
    [${ERROR_ATTR}] {
      position: fixed;
      top: 16px;
      right: 16px;
      z-index: 60;
      width: min(28rem, calc(100vw - 32px));
    }
  `;
}

const WARNING_ICON = `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" fill="currentColor" viewBox="0 0 16 16" class="icon-xs" aria-hidden="true"><path d="M8 9.8a.767.767 0 1 1 0 1.533A.767.767 0 0 1 8 9.8Zm0-5.134c.368 0 .667.299.667.667V8a.667.667 0 0 1-1.334 0V5.333c0-.368.299-.667.667-.667Z"/><path fill-rule="evenodd" d="M8 1.333a6.667 6.667 0 1 1 0 13.334A6.667 6.667 0 0 1 8 1.333Zm0 1.334a5.333 5.333 0 1 0 0 10.666A5.333 5.333 0 0 0 8 2.667Z" clip-rule="evenodd"/></svg>`;

function hideLaunchError(): void {
  document.querySelector<HTMLElement>(`[${ERROR_ATTR}]`)?.remove();
}

function showLaunchError(): void {
  hideLaunchError();
  const card = document.createElement("div");
  card.setAttribute(ERROR_ATTR, "true");
  card.setAttribute("role", "alert");
  card.className =
    "alert-root inline-flex flex-col gap-2 rounded-xl px-2 py-2 text-base leading-[1.4] pointer-events-auto box-shadow-lg border border-warning-outline bg-warning-surface text-warning";

  const row = document.createElement("div");
  row.className = "flex min-w-0 items-start gap-1";

  const iconWrap = document.createElement("div");
  iconWrap.className = "flex size-6 shrink-0 grow-0 items-center justify-center self-start";
  iconWrap.innerHTML = WARNING_ICON;

  const mid = document.createElement("div");
  mid.className = "flex min-w-0 flex-1 items-start gap-3";
  const copy = document.createElement("div");
  copy.className = "min-w-0 flex-1 justify-center gap-2 break-words";
  const title = document.createElement("div");
  title.className = "flex min-h-6 items-center text-start font-medium whitespace-pre-wrap";
  title.textContent = t("errorTitle");
  const body = document.createElement("div");
  body.className = "text-start text-warning/80";
  body.textContent = t("errorBody");
  copy.append(title, body);

  const actions = document.createElement("div");
  actions.className = "flex shrink-0 items-center gap-2";
  const retry = document.createElement("button");
  retry.type = "button";
  retry.className =
    "shrink-0 rounded-full bg-primary-solid px-3 py-1 text-sm font-medium text-primary-solid";
  retry.textContent = t("errorRetry");
  retry.addEventListener("click", () => {
    hideLaunchError();
    void activate();
  });
  actions.append(retry);
  mid.append(copy, actions);

  const close = document.createElement("button");
  close.type = "button";
  close.setAttribute("aria-label", t("errorClose"));
  close.className =
    "flex size-6 shrink-0 grow-0 cursor-interaction items-center justify-center self-start rounded-full hover:bg-background-primary-ghost-hover/5";
  close.innerHTML = CLOSE_SVG;
  close.addEventListener("click", () => hideLaunchError());

  row.append(iconWrap, mid, close);
  card.append(row);
  document.body.append(card);
}

function findSearchButton(): HTMLElement | null {
  return (
    [...document.querySelectorAll<HTMLElement>("button")].find((btn) =>
      isSearchLabel(btn.getAttribute("aria-label")),
    ) ?? null
  );
}

function isParkedLeftOfSearch(btn: HTMLElement, search: HTMLElement): boolean {
  const placement = searchButtonPlacement(search);
  return Boolean(
    placement && btn.parentElement === placement.parent && btn.nextElementSibling === placement.before,
  );
}

function buttonStillBesideSearch(): boolean {
  const btn = document.querySelector<HTMLElement>(`[${BTN_ATTR}]`);
  const search = findSearchButton();
  return Boolean(btn?.isConnected && search && isParkedLeftOfSearch(btn, search));
}

function injectedTooltipCanShow(btn: HTMLElement): boolean {
  const search = findSearchButton();
  return (
    btn.isConnected &&
    (btn.getAttribute("data-incodex-hovered") === "true" || document.activeElement === btn) &&
    !(search && searchTooltipOpen(search))
  );
}

function landingStillMounted(): boolean {
  const landing = document.querySelector(`[${LANDING_ATTR}]`);
  if (!isIncognitoWindow() || bannerDismissed()) return !landing;
  return Boolean(landing);
}

function needsInject(): boolean {
  return !buttonStillBesideSearch() || !landingStillMounted();
}

function buildButton(search: HTMLElement): HTMLElement {
  disposeActiveTooltip();
  const btn = search.cloneNode(false) as HTMLElement;
  for (const name of STRIP_CLONE_ATTRS) btn.removeAttribute(name);
  for (const name of [...btn.attributes].map((attr) => attr.name)) {
    if (name.startsWith("data-") && name !== BTN_ATTR) btn.removeAttribute(name);
  }
  btn.setAttribute("type", "button");
  btn.setAttribute(BTN_ATTR, "true");
  btn.setAttribute("data-incodex-hovered", "false");
  btn.className = search.className;
  const svg = createButtonIcon(ICON_SVG, "hat-glasses", search.querySelector("svg"));
  if (svg) btn.append(svg);
  const tooltipLifecycle = createTooltipLifecycle({
    delayMs: TOOLTIP_FALLBACK_DELAY_MS,
    schedule: (callback, delayMs) => window.setTimeout(callback, delayMs),
    cancel: (id) => window.clearTimeout(id),
    canShow: () => injectedTooltipCanShow(btn),
    show: () => showTooltip(btn),
    hide: hideTooltip,
  });
  activeTooltipLifecycle = tooltipLifecycle;
  btn.addEventListener(
    "click",
    (event) => {
      event.preventDefault();
      event.stopImmediatePropagation();
      void activate();
    },
    true,
  );
  btn.addEventListener("pointerenter", () => {
    setButtonHover(btn, true);
    tooltipLifecycle.pointerEnter();
  });
  btn.addEventListener("pointerleave", () => {
    setButtonHover(btn, false);
    tooltipLifecycle.pointerLeave();
  });
  btn.addEventListener("focus", tooltipLifecycle.focus);
  btn.addEventListener("blur", tooltipLifecycle.blur);
  return btn;
}

function tooltipEl(): HTMLElement {
  let tip = document.querySelector<HTMLElement>(`[${TIP_ATTR}]`);
  if (tip) return tip;
  tip = document.createElement("div");
  tip.setAttribute(TIP_ATTR, "true");
  tip.setAttribute("role", "tooltip");
  tip.className =
    "z-50 w-fit select-none text-sm whitespace-normal break-words rounded-lg border border-text bg-primary-solid text-primary-solid px-2 py-1.5";
  const text = document.createElement("div");
  text.className = "flex items-center gap-2";
  const label = document.createElement("div");
  label.className = "min-w-0";
  label.setAttribute("data-incodex-tooltip-label", "true");
  const kbd = document.createElement("kbd");
  kbd.className =
    "inline-flex !rounded-md !border-0 !bg-current/10 !font-sans !text-xs !text-current !shadow-none !px-1.5 !py-0.5 !leading-none";
  kbd.textContent = SHORTCUT_LABEL;
  text.append(label, kbd);
  tip.append(text);
  document.body.append(tip);
  return tip;
}

// Official header tooltips use side=top, sideOffset=2. Pin our bottom edge
// to that same gap so a taller label still lines up with Search / Info.
const TOOLTIP_SIDE_OFFSET = 2;

function showTooltip(btn: HTMLElement): void {
  const tip = tooltipEl();
  const label = tip.querySelector<HTMLElement>("[data-incodex-tooltip-label]");
  if (label) label.textContent = labelFor(btn.getAttribute("aria-pressed") === "true");
  tip.style.visibility = "hidden";
  tip.setAttribute("data-open", "true");
  const rect = btn.getBoundingClientRect();
  const tipRect = tip.getBoundingClientRect();
  const left = Math.min(
    window.innerWidth - tipRect.width - 8,
    Math.max(8, rect.left + rect.width / 2 - tipRect.width / 2),
  );
  tip.style.left = `${left}px`;
  tip.style.top = "auto";
  tip.style.bottom = `${Math.max(8, window.innerHeight - rect.top + TOOLTIP_SIDE_OFFSET)}px`;
  tip.style.visibility = "";
}

function hideTooltip(): void {
  const tip = document.querySelector<HTMLElement>(`[${TIP_ATTR}]`);
  if (!tip) return;
  tip.removeAttribute("data-open");
  tip.style.bottom = "";
  tip.style.top = "";
}

const BANNER_DISMISS_KEY = "incodex-banner-dismissed";
const BANNER_HOST_ATTR = "data-incodex-banner-host";
const BANNER_TITLE_ATTR = "data-incodex-banner-title";
const BANNER_BODY_ATTR = "data-incodex-banner-body";
const CLOSE_SVG = `<svg width="16" height="16" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg" class="icon-xs" aria-hidden="true"><path d="M4.2 4.2l7.6 7.6M11.8 4.2l-7.6 7.6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>`;

function bannerDismissed(): boolean {
  try {
    return window.sessionStorage.getItem(BANNER_DISMISS_KEY) === "1";
  } catch {
    return false;
  }
}

function refreshUiProbe(): void {
  const incognito = isIncognitoWindow();
  window.__incodexUiProbe = deriveUiProbe({
    incognito,
    buttonPresent: buttonStillBesideSearch(),
    bannerPresent: Boolean(
      document.querySelector(`[${BANNER_HOST_ATTR}]`)?.querySelector(`[${LANDING_ATTR}]`),
    ),
    bannerDismissed: incognito && bannerDismissed(),
  });
}

function dismissBanner(): void {
  try {
    window.sessionStorage.setItem(BANNER_DISMISS_KEY, "1");
  } catch {
    /* ignore */
  }
  document.querySelector<HTMLElement>(`[${LANDING_ATTR}]`)?.closest(`[${BANNER_HOST_ATTR}]`)?.remove();
  document.querySelector<HTMLElement>(`[${LANDING_ATTR}]`)?.remove();
  refreshUiProbe();
}

function classNameOf(el: Element): string {
  const value = el.getAttribute("class") || "";
  return typeof value === "string" ? value : "";
}

function findOfficialBannerSlot(): HTMLElement | null {
  return (
    [...document.querySelectorAll<HTMLElement>("div")].find((el) => {
      if (el.hasAttribute(BANNER_HOST_ATTR)) return false;
      return classNameOf(el).split(/\s+/).includes("home-banners");
    }) ?? null
  );
}

function findLandingMount(): { parent: HTMLElement; before: Node | null } | null {
  const slot = findOfficialBannerSlot();
  if (slot) return { parent: slot, before: slot.firstChild };
  return null;
}

function buildLanding(): HTMLElement {
  const host = document.createElement("div");
  host.setAttribute(BANNER_HOST_ATTR, "true");
  host.className = "home-banners flex w-full min-w-0 flex-col gap-2 pt-2";

  const card = document.createElement("aside");
  card.setAttribute(LANDING_ATTR, "true");
  card.setAttribute("aria-live", "polite");
  card.className =
    "relative isolate flex w-full items-center gap-4 overflow-hidden rounded-2xl border border-primary-outline bg-surface py-2 ps-3 pe-2 text-sm text-default shadow-xs lg:mx-auto electron:border-0 electron:ring-[0.5px] electron:ring-border-strong";

  const wash = document.createElement("div");
  wash.setAttribute("aria-hidden", "true");
  wash.className = "absolute inset-0 -z-10 bg-primary-soft";

  const row = document.createElement("div");
  row.className = "flex h-full w-full min-w-0 items-center gap-2";

  const visual = document.createElement("div");
  visual.className = "flex size-12 shrink-0 items-center justify-center self-center text-secondary";
  visual.innerHTML = ICON_SVG.trim();
  const svg = visual.querySelector("svg");
  if (svg) {
    svg.setAttribute("class", "icon-sm");
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("width", "24");
    svg.setAttribute("height", "24");
  }

  const copy = document.createElement("div");
  copy.className = "min-w-0 flex-1";
  const titleWrap = document.createElement("div");
  titleWrap.className = "flex flex-wrap items-center gap-2";
  const title = document.createElement("div");
  title.className = "min-w-0 text-base font-medium text-default";
  title.setAttribute(BANNER_TITLE_ATTR, "true");
  title.textContent = t("title");
  titleWrap.append(title);
  const body = document.createElement("div");
  body.className = "text-sm leading-tight text-pretty text-secondary";
  body.setAttribute(BANNER_BODY_ATTR, "true");
  body.textContent = t("body");
  copy.append(titleWrap, body);

  const actions = document.createElement("div");
  actions.className =
    "flex items-center gap-2 self-center max-[400px]:w-full max-[400px]:justify-center max-[400px]:self-stretch";
  const close = document.createElement("button");
  close.type = "button";
  close.setAttribute("aria-label", t("dismiss"));
  close.className =
    "flex size-8 shrink-0 items-center justify-center rounded-lg border-transparent text-codex-description hover:text-default";
  close.innerHTML = CLOSE_SVG;
  close.addEventListener(
    "click",
    (event) => {
      event.preventDefault();
      event.stopPropagation();
      dismissBanner();
    },
    true,
  );
  actions.append(close);

  row.append(visual, copy, actions);
  card.append(wash, row);
  host.append(card);
  return host;
}

function syncLandingCopy(host: HTMLElement): void {
  const title = host.querySelector<HTMLElement>(`[${BANNER_TITLE_ATTR}]`);
  const body = host.querySelector<HTMLElement>(`[${BANNER_BODY_ATTR}]`);
  const close = host.querySelector<HTMLButtonElement>("button[aria-label]");
  if (title) title.textContent = t("title");
  if (body) body.textContent = t("body");
  if (close) close.setAttribute("aria-label", t("dismiss"));
}

function ensureLanding(): void {
  if (!isIncognitoWindow()) {
    document.querySelector<HTMLElement>(`[${BANNER_HOST_ATTR}]`)?.remove();
    document.querySelector<HTMLElement>(`[${LANDING_ATTR}]`)?.remove();
    return;
  }
  if (bannerDismissed()) {
    document.querySelector<HTMLElement>(`[${BANNER_HOST_ATTR}]`)?.remove();
    document.querySelector<HTMLElement>(`[${LANDING_ATTR}]`)?.remove();
    return;
  }

  const mount = findLandingMount();
  if (!mount) return;

  let host = document.querySelector<HTMLElement>(`[${BANNER_HOST_ATTR}]`);
  if (!host) host = buildLanding();
  syncLandingCopy(host);

  const officialSlot = findOfficialBannerSlot();
  if (officialSlot) {
    host.className = "";
    if (officialSlot.firstElementChild !== host) officialSlot.insertBefore(host, officialSlot.firstChild);
    return;
  }
  if (mount.before === host) return;
  if (host.parentElement === mount.parent && host.nextSibling === mount.before) return;
  host.className = "home-banners flex w-full min-w-0 flex-col gap-2 pt-2";
  mount.parent.insertBefore(host, mount.before);
}

function ensureButton(): void {
  let btn = document.querySelector<HTMLElement>(`[${BTN_ATTR}]`);
  const search = findSearchButton();
  const placement = search ? searchButtonPlacement(search) : null;
  if (!search || !placement) {
    if (btn?.isConnected) dismissActiveTooltip();
    else disposeActiveTooltip();
    return;
  }

  if (!btn) btn = buildButton(search);
  if (!isParkedLeftOfSearch(btn, search)) {
    placement.parent.insertBefore(btn, placement.before);
  }
  apply();
}

function onKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    dismissActiveTooltip();
    return;
  }
  if (!(event.metaKey || event.ctrlKey) || !event.shiftKey) return;
  if (event.code !== "KeyN" && event.key.toLowerCase() !== "n") return;
  event.preventDefault();
  event.stopImmediatePropagation();
  void activate();
}

function start(): void {
  if (window.__incodexStarted) {
    refreshUiProbe();
    return;
  }
  window.__incodexStarted = true;
  ensureStyle();
  ensureButton();
  apply();
  ensureLanding();
  refreshUiProbe();
  window.addEventListener("keydown", onKeydown, true);
  window.addEventListener("blur", dismissActiveTooltip);
  window.addEventListener(TOOLTIP_DISMISS_EVENT, () => activeTooltipLifecycle?.dismiss());
  let scheduled = false;
  const observer = new MutationObserver(() => {
    if (!needsInject()) return;
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(() => {
      scheduled = false;
      if (!needsInject()) return;
      ensureButton();
      ensureLanding();
      refreshUiProbe();
    });
  });
  observer.observe(document.documentElement, { childList: true, subtree: true });
}

declare global {
  interface Window {
    __incodexStarted?: boolean;
    __incodexIncognito?: boolean;
    __incodexLocale?: string;
    __incodexUiProbe?: ReturnType<typeof deriveUiProbe>;
    incodex?: {
      requestIncognitoAction?: (payload: {
        action: "open" | "quit";
        requestId: string;
      }) => Promise<{ ok: boolean; reason?: string; code?: string; requestId?: string }>;
    };
  }
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", start, { once: true });
} else {
  start();
}
