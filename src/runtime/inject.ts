import { isSearchLabel } from "./compatibility/search-labels.ts";
import { deriveUiProbe } from "./incodex-ui-probe.ts";
import { resolveLocale as matchLocale, translate, type CopyKey } from "./incognito-copy.ts";
import {
  ensureProfileMask,
  profileMaskHealth,
  profileMaskNeedsInject,
} from "./incognito-profile-mask.ts";
import { createOfficialTooltipTimingBridge } from "./official-tooltip-provider.ts";
import { searchButtonPlacement, searchTooltipOpen } from "./search-button-placement.ts";
import { createTooltipLifecycle, type TooltipLifecycle } from "./tooltip-lifecycle.ts";
import { officialWindowZoom } from "./tooltip-presentation.ts";

const STYLE_ID = "incodex-privacy-style";
const BTN_ATTR = "data-incodex-privacy-toggle";
const TIP_ATTR = "data-incodex-tooltip";
const TIP_HOST_ATTR = "data-incodex-tooltip-host";
const LANDING_ATTR = "data-incodex-landing";
const ERROR_ATTR = "data-incodex-launch-error";
const ERROR_OVERLAY_ATTR = "data-incodex-launch-error-overlay";
const SHORTCUT_LABEL = "⇧⌘N";
const TOOLTIP_FALLBACK_DELAY_MS = 700;
const TOOLTIP_DISMISS_EVENT = "codex:dismiss-tooltips";

type IncognitoAction = "open" | "quit";
type IncognitoButtonIcon = "hat-glasses" | "circle-x";

type IncognitoActionResponse = {
  code?: string;
  ok: boolean;
  reason?: string;
  requestId?: string;
};

const STRIP_CLONE_ATTRS = [
  "id",
  "name",
  "aria-haspopup",
  "aria-expanded",
  "aria-controls",
  "aria-describedby",
  "aria-labelledby",
  "data-state",
  "data-testid",
  "data-test-id",
  "disabled",
  "title",
  "tabindex",
];

let activeTooltipLifecycle: TooltipLifecycle | null = null;
let launchErrorPending = false;
let windowsLaunchErrorHost: HTMLElement | null = null;

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

function isWindowsRenderer(): boolean {
  return window.__incodexPlatform === "win32";
}

function shortcutLabel(): string {
  return isWindowsRenderer() ? "Ctrl+Shift+N" : SHORTCUT_LABEL;
}

function currentLocale(): string {
  const locale =
    window.__incodexLocale || document.documentElement.lang || navigator.language || "en";
  return matchLocale(locale);
}

function t(key: CopyKey): string {
  return translate(currentLocale(), key);
}

function labelFor(on: boolean): string {
  return on ? t("exit") : t("open");
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
  const name: IncognitoButtonIcon =
    isIncognitoWindow() && btn.getAttribute("data-incodex-hovered") === "true"
      ? "circle-x"
      : "hat-glasses";
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

async function requestAction(action: IncognitoAction): Promise<IncognitoActionResponse> {
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

async function activate(): Promise<boolean> {
  dismissActiveTooltip();
  if (isIncognitoWindow()) {
    const result = await requestAction("quit");
    if (!result.ok) window.close();
    return true;
  }
  const result = await requestAction("open");
  if (result.ok) {
    hideLaunchError();
    return true;
  }
  showLaunchError();
  return false;
}

function ensureStyle(): void {
  let style = document.getElementById(STYLE_ID) as HTMLStyleElement | null;
  if (!style) {
    style = document.createElement("style");
    style.id = STYLE_ID;
    document.head.append(style);
  }
  style.textContent = `
    [${TIP_HOST_ATTR}] {
      position: fixed;
      z-index: 50;
      display: none;
      pointer-events: none !important;
    }
    [${TIP_HOST_ATTR}][data-open="true"] { display: block; }
    [${TIP_ATTR}] {
      max-width: min(20rem, calc(100vw - 16px));
      pointer-events: none !important;
      user-select: none;
      box-sizing: border-box;
    }
    [${ERROR_OVERLAY_ATTR}] {
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
  launchErrorPending = false;
  windowsLaunchErrorHost?.remove();
  windowsLaunchErrorHost = null;
  document.querySelector<HTMLElement>(`[${ERROR_ATTR}]`)?.remove();
}

function showLaunchError(): void {
  hideLaunchError();
  if (isWindowsRenderer()) {
    launchErrorPending = true;
    ensureLaunchError();
    return;
  }
  const card = document.createElement("div");
  card.setAttribute(ERROR_ATTR, "true");
  card.setAttribute(ERROR_OVERLAY_ATTR, "true");
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

function tooltipMountStillPresent(): boolean {
  const host = document.querySelector<HTMLElement>(`[${TIP_HOST_ATTR}]`);
  const tip = host?.querySelector<HTMLElement>(`[${TIP_ATTR}]`);
  return Boolean(host?.isConnected && tip?.isConnected && tip.parentElement === host);
}

function needsInject(): boolean {
  return (
    !buttonStillBesideSearch() ||
    !tooltipMountStillPresent() ||
    !landingStillMounted() ||
    launchErrorNeedsInject() ||
    profileMaskNeedsInject()
  );
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
  const providerTiming = createOfficialTooltipTimingBridge(findSearchButton);
  const tooltipLifecycle: TooltipLifecycle = createTooltipLifecycle({
    delayMs: TOOLTIP_FALLBACK_DELAY_MS,
    resolveDelay: providerTiming.resolveDelay,
    schedule: (callback, delayMs) => window.setTimeout(callback, delayMs),
    cancel: (id) => window.clearTimeout(id),
    canShow: () => injectedTooltipCanShow(btn),
    onOpen: providerTiming.activate,
    onClose: providerTiming.deactivate,
    show: () => showTooltip(btn),
    hide: hideTooltip,
  });
  activeTooltipLifecycle = tooltipLifecycle;
  btn.addEventListener(
    "click",
    (event) => {
      event.preventDefault();
      event.stopImmediatePropagation();
      setButtonHover(btn, false);
      tooltipLifecycle.trigger();
      btn.blur();
      void activate().then((completed) => {
        if (!completed && btn.isConnected) btn.focus();
      });
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

function createTooltipElement(): HTMLElement {
  const tip = document.createElement("div");
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
  kbd.textContent = shortcutLabel();
  text.append(label, kbd);
  tip.append(text);
  return tip;
}

function ensureTooltipMount(): HTMLElement {
  let host = document.querySelector<HTMLElement>(`[${TIP_HOST_ATTR}]`);
  if (!host) {
    host = document.createElement("div");
    host.setAttribute(TIP_HOST_ATTR, "true");
    document.body.append(host);
  }

  let tip = host.querySelector<HTMLElement>(`[${TIP_ATTR}]`);
  if (!tip) {
    tip = document.querySelector<HTMLElement>(`[${TIP_ATTR}]`) ?? createTooltipElement();
    if (tip.parentElement !== host) host.append(tip);
  }
  return tip;
}

function tooltipEl(): HTMLElement {
  return ensureTooltipMount();
}

// Official header tooltips use side=top, sideOffset=2. Pin our bottom edge
// to that same gap so a taller label still lines up with Search / Info.
const TOOLTIP_SIDE_OFFSET = 2;

function showTooltip(btn: HTMLElement): void {
  const tip = tooltipEl();
  const host = tip.parentElement;
  if (!host) return;
  const label = tip.querySelector<HTMLElement>("[data-incodex-tooltip-label]");
  if (label) label.textContent = labelFor(btn.getAttribute("aria-pressed") === "true");
  const zoom = officialWindowZoom(document.documentElement);
  tip.style.zoom = zoom === 1 ? "" : String(zoom);
  host.style.visibility = "hidden";
  host.setAttribute("data-open", "true");
  const rect = btn.getBoundingClientRect();
  const tipRect = tip.getBoundingClientRect();
  const left = Math.min(
    window.innerWidth - tipRect.width - 8,
    Math.max(8, rect.left + rect.width / 2 - tipRect.width / 2),
  );
  host.style.left = `${left}px`;
  host.style.top = "auto";
  host.style.bottom = `${Math.max(8, window.innerHeight - rect.top + TOOLTIP_SIDE_OFFSET)}px`;
  host.style.visibility = "";
}

function hideTooltip(): void {
  const host = document.querySelector<HTMLElement>(`[${TIP_HOST_ATTR}]`);
  if (!host) return;
  host.removeAttribute("data-open");
  host.style.bottom = "";
  host.style.left = "";
  host.style.top = "";
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
  window.__incodexProfileMaskHealth = profileMaskHealth();
  window.__incodexUiProbe = deriveUiProbe({
    incognito,
    buttonPresent: buttonStillBesideSearch(),
    tooltipPresent: tooltipMountStillPresent(),
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

function classNameOf(element: Element): string {
  return element.getAttribute("class") ?? "";
}

function findOfficialBannerSlot(): HTMLElement | null {
  return (
    [...document.querySelectorAll<HTMLElement>("div")].find((el) => {
      if (el.hasAttribute(BANNER_HOST_ATTR)) return false;
      return classNameOf(el).split(/\s+/).includes("home-banners");
    }) ?? null
  );
}

function mountInOfficialBannerSlot(element: HTMLElement): boolean {
  const slot = findOfficialBannerSlot();
  if (!slot) return false;
  if (slot.firstElementChild !== element) slot.insertBefore(element, slot.firstChild);
  return true;
}

type OfficialHomeBannerOptions = {
  body: string;
  cardAttribute?: string;
  closeLabel: string;
  hostAttribute: string;
  icon: string;
  iconSize: number;
  onClose: () => void;
  primaryAction?: { label: string; onClick: () => void };
  title: string;
  warning?: boolean;
};

function cloneOfficialPrimaryAction(): HTMLButtonElement | null {
  const slot = findOfficialBannerSlot();
  const source =
    [...(slot?.querySelectorAll<HTMLButtonElement>("button") ?? [])].find(
      (button) =>
        button.textContent?.trim() &&
        !button.closest(`[${BANNER_HOST_ATTR}]`) &&
        !button.closest(`[${ERROR_ATTR}]`),
    ) ?? document.querySelector<HTMLButtonElement>("button.bg-primary-solid");
  if (!source) return null;
  const clone = source.cloneNode(false) as HTMLButtonElement;
  for (const name of STRIP_CLONE_ATTRS) clone.removeAttribute(name);
  for (const name of [...clone.attributes].map((attribute) => attribute.name)) {
    if (name.startsWith("data-")) clone.removeAttribute(name);
  }
  clone.type = "button";
  clone.disabled = false;
  return clone;
}

function buildOfficialHomeBanner(options: OfficialHomeBannerOptions): HTMLElement {
  const host = document.createElement("div");
  host.setAttribute(options.hostAttribute, "true");

  const card = document.createElement("aside");
  if (options.cardAttribute) card.setAttribute(options.cardAttribute, "true");
  card.setAttribute("aria-live", options.warning ? "assertive" : "polite");
  if (options.warning) card.setAttribute("role", "alert");
  card.className =
    `relative isolate flex w-full items-center gap-4 overflow-hidden rounded-2xl border bg-surface py-2 ps-3 pe-2 text-sm text-default shadow-xs lg:mx-auto electron:border-0 electron:ring-[0.5px] electron:ring-border-strong ${
      options.warning ? "border-text-warning/30" : "border-primary-outline"
    }`;

  const wash = document.createElement("div");
  wash.setAttribute("aria-hidden", "true");
  wash.className = `absolute inset-0 -z-10 ${
    options.warning ? "bg-background-warning-surface/30" : "bg-primary-soft"
  }`;

  const row = document.createElement("div");
  row.className = "flex h-full w-full min-w-0 items-center gap-2";

  const visual = document.createElement("div");
  visual.className = `flex size-12 shrink-0 items-center justify-center self-center ${
    options.warning ? "text-warning" : "text-secondary"
  }`;
  visual.innerHTML = options.icon.trim();
  const svg = visual.querySelector("svg");
  if (svg) {
    svg.setAttribute("class", "icon-sm");
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("width", String(options.iconSize));
    svg.setAttribute("height", String(options.iconSize));
  }

  const copy = document.createElement("div");
  copy.className = "min-w-0 flex-1";
  const titleWrap = document.createElement("div");
  titleWrap.className = "flex flex-wrap items-center gap-2";
  const title = document.createElement("div");
  title.className = "min-w-0 text-base font-medium text-default";
  title.setAttribute(BANNER_TITLE_ATTR, "true");
  title.textContent = options.title;
  titleWrap.append(title);
  const body = document.createElement("div");
  body.className = "text-sm leading-tight text-pretty text-secondary";
  body.setAttribute(BANNER_BODY_ATTR, "true");
  body.textContent = options.body;
  copy.append(titleWrap, body);

  const actions = document.createElement("div");
  actions.className =
    "flex items-center gap-2 self-center max-[400px]:w-full max-[400px]:justify-center max-[400px]:self-stretch";
  if (options.primaryAction) {
    const primary = cloneOfficialPrimaryAction() ?? document.createElement("button");
    primary.type = "button";
    if (!primary.className) {
      primary.className =
        "shrink-0 rounded-full bg-primary-solid px-3 py-1 text-sm font-medium text-primary-solid";
    }
    primary.textContent = options.primaryAction.label;
    primary.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      options.primaryAction?.onClick();
    });
    actions.append(primary);
  }
  const close = document.createElement("button");
  close.type = "button";
  close.setAttribute("aria-label", options.closeLabel);
  close.className =
    "flex size-8 shrink-0 items-center justify-center rounded-lg border-transparent text-codex-description hover:text-default";
  close.innerHTML = CLOSE_SVG;
  close.addEventListener(
    "click",
    (event) => {
      event.preventDefault();
      event.stopPropagation();
      options.onClose();
    },
    true,
  );
  actions.append(close);

  row.append(visual, copy, actions);
  card.append(wash, row);
  host.append(card);
  return host;
}

function buildLanding(): HTMLElement {
  return buildOfficialHomeBanner({
    body: t("body"),
    cardAttribute: LANDING_ATTR,
    closeLabel: t("dismiss"),
    hostAttribute: BANNER_HOST_ATTR,
    icon: ICON_SVG,
    iconSize: 24,
    onClose: dismissBanner,
    title: t("title"),
  });
}

function buildWindowsLaunchErrorBanner(): HTMLElement {
  return buildOfficialHomeBanner({
    body: t("errorBody"),
    closeLabel: t("errorClose"),
    hostAttribute: ERROR_ATTR,
    icon: WARNING_ICON,
    iconSize: 20,
    onClose: hideLaunchError,
    primaryAction: {
      label: t("errorRetry"),
      onClick: () => {
        hideLaunchError();
        void activate();
      },
    },
    title: t("errorTitle"),
    warning: true,
  });
}

function launchErrorNeedsInject(): boolean {
  if (!isWindowsRenderer() || !launchErrorPending) return false;
  const slot = findOfficialBannerSlot();
  return !slot || !windowsLaunchErrorHost?.isConnected || windowsLaunchErrorHost.parentElement !== slot;
}

function ensureLaunchError(): void {
  if (!isWindowsRenderer() || !launchErrorPending) return;
  if (!windowsLaunchErrorHost) windowsLaunchErrorHost = buildWindowsLaunchErrorBanner();
  windowsLaunchErrorHost.className = "";
  mountInOfficialBannerSlot(windowsLaunchErrorHost);
}

function syncLandingCopy(host: HTMLElement): void {
  const title = host.querySelector<HTMLElement>(`[${BANNER_TITLE_ATTR}]`);
  const body = host.querySelector<HTMLElement>(`[${BANNER_BODY_ATTR}]`);
  const close = host.querySelector<HTMLButtonElement>("button[aria-label]");
  if (title) title.textContent = t("title");
  if (body) body.textContent = t("body");
  if (close) close.setAttribute("aria-label", t("dismiss"));
}

function removeLanding(): void {
  document.querySelector<HTMLElement>(`[${BANNER_HOST_ATTR}]`)?.remove();
  document.querySelector<HTMLElement>(`[${LANDING_ATTR}]`)?.remove();
}

function ensureLanding(): void {
  if (!isIncognitoWindow() || bannerDismissed()) {
    removeLanding();
    return;
  }

  let host = document.querySelector<HTMLElement>(`[${BANNER_HOST_ATTR}]`);
  if (!host) host = buildLanding();
  syncLandingCopy(host);
  host.className = "";
  mountInOfficialBannerSlot(host);
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
  ensureTooltipMount();
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

const PROFILE_OBSERVED_ATTRIBUTES = [
  "aria-controls",
  "class",
  "src",
  "style",
  "data-incodex-profile-mask",
  "data-incodex-profile-mask-name",
  "data-incodex-profile-mask-avatar",
];

function observerOptions(): MutationObserverInit {
  const options: MutationObserverInit = { childList: true, subtree: true };
  if (profileObservationRequired()) {
    options.attributes = true;
    options.characterData = true;
    options.attributeFilter = PROFILE_OBSERVED_ATTRIBUTES;
  }
  return options;
}

function profileObservationRequired(): boolean {
  return (
    isIncognitoWindow() &&
    window.__incodexProfileMask !== null &&
    window.__incodexProfileMask !== undefined
  );
}

function createMutationObserver(): MutationObserver {
  let scheduled = false;
  return new MutationObserver(function handleMutation(): void {
    if (!needsInject() || scheduled) return;
    scheduled = true;
    requestAnimationFrame(function injectOnAnimationFrame(): void {
      scheduled = false;
      if (!needsInject()) return;
      ensureButton();
      ensureLanding();
      ensureLaunchError();
      ensureProfileMask();
      refreshUiProbe();
    });
  });
}

function ensureMutationObserver(): void {
  const profileRequired = profileObservationRequired();
  let observer = window.__incodexMutationObserver;
  if (!observer) {
    observer = createMutationObserver();
    window.__incodexMutationObserver = observer;
  }
  observer.observe(document.documentElement, observerOptions());
  window.__incodexProfileObservationEnabled = profileRequired;
}

function start(): void {
  if (window.__incodexStarted) {
    ensureStyle();
    ensureButton();
    ensureLanding();
    ensureLaunchError();
    ensureProfileMask();
    refreshUiProbe();
    ensureMutationObserver();
    return;
  }
  window.__incodexStarted = true;
  ensureStyle();
  ensureButton();
  apply();
  ensureLanding();
  ensureLaunchError();
  ensureProfileMask();
  refreshUiProbe();
  window.addEventListener("keydown", onKeydown, true);
  window.addEventListener("blur", () => activeTooltipLifecycle?.windowBlur());
  window.addEventListener("focus", () => activeTooltipLifecycle?.windowFocus());
  window.addEventListener(TOOLTIP_DISMISS_EVENT, () => activeTooltipLifecycle?.dismiss());
  ensureMutationObserver();
}

declare global {
  interface Window {
    __incodexStarted?: boolean;
    __incodexIncognito?: boolean;
    __incodexLocale?: string;
    __incodexPlatform?: string;
    __incodexMutationObserver?: MutationObserver;
    __incodexProfileObservationEnabled?: boolean;
    __incodexProfileMaskHealth?: boolean;
    __incodexRefreshProfileMaskHealth?: () => boolean;
    __incodexUiProbe?: ReturnType<typeof deriveUiProbe>;
    incodex?: {
      requestIncognitoAction?: (payload: {
        action: IncognitoAction;
        requestId: string;
      }) => Promise<IncognitoActionResponse>;
    };
  }
}

window.__incodexRefreshProfileMaskHealth = profileMaskHealth;

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", start, { once: true });
} else {
  start();
}
