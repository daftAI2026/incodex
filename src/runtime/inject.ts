import { createOfficialNotifications, loadOfficialBannerModules } from "./official-notifications.ts";
import { cloneButtonIconLayout } from "./button-icon-layout.ts";
import { isSearchLabel } from "./compatibility/search-labels.ts";
import { deriveUiProbe } from "./incodex-ui-probe.ts";
import { resolveLocale as matchLocale, translate, type CopyKey } from "./incognito-copy.ts";
import {
  ensureProfileMask,
  profileMaskHealth,
  profileMaskNeedsInject,
  refreshProfileMaskHealth,
} from "./incognito-profile-mask.ts";
import { createOfficialTooltipTimingBridge } from "./official-tooltip-provider.ts";
import { createOfficialModuleSourceReader, createOfficialTooltipRenderer, loadOfficialTooltipModules, sharedTooltipState } from "./official-tooltip-renderer.ts";
import { officialStyleAttributes, syncOfficialButtonAppearance } from "./official-style-attributes.ts";
import { searchButtonPlacement, searchTooltipOpen } from "./search-button-placement.ts";
import { createTooltipLifecycle, type TooltipLifecycle } from "./tooltip-lifecycle.ts";
import {
  createOfficialTooltipPresentation,
  findOfficialTooltipElement,
  officialWindowZoom,
} from "./tooltip-presentation.ts";

const STYLE_ID = "incodex-privacy-style";
const BTN_ATTR = "data-incodex-privacy-toggle";
const TIP_ATTR = "data-incodex-tooltip";
const TIP_HOST_ATTR = "data-incodex-tooltip-host";
const SHORTCUT_LABEL = "⇧⌘N";
const TOOLTIP_FALLBACK_DELAY_MS = 700;
const TOOLTIP_DISMISS_EVENT = "codex:dismiss-tooltips";

type IncognitoAction = "open" | "quit";
type IncognitoBridgeAction =
  | IncognitoAction
  | "configure-dock-menu"
  | "configure-status-menu";
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

const tooltipState = sharedTooltipState(window);
const readOfficialSource = createOfficialModuleSourceReader();
const officialTooltipPresentation = createOfficialTooltipPresentation();
const notifications = window.__incodexNotifications ??= createOfficialNotifications(document, () =>
  loadOfficialBannerModules(document, () => {
    // 横幅复用本窗口已验证或正在准备的 React 能力，不重复发现。
    const renderer = tooltipState.renderer;
    return typeof renderer?.preparedModules === "function"
      ? renderer.preparedModules()
      : loadOfficialTooltipModules(document, readOfficialSource);
  }, undefined, readOfficialSource),
);

function dismissActiveTooltip(): void {
  tooltipState.lifecycle?.dismiss();
}

function disposeActiveTooltip(): void {
  tooltipState.lifecycle?.dispose();
  tooltipState.lifecycle = null;
  tooltipState.renderer?.dispose();
  tooltipState.renderer = null;
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

const buttonStyleAttributes = new WeakMap<HTMLElement, ReadonlySet<string>>();

function createButtonIcon(
  source: string,
  name: IncognitoButtonIcon,
  sample: SVGElement | null,
  styleAttributes: ReadonlySet<string>,
): SVGElement | null {
  const wrap = document.createElement("span");
  wrap.innerHTML = source.trim();
  const svg = wrap.firstElementChild as SVGElement | null;
  if (!svg) return null;
  svg.setAttribute("data-incodex-icon", name);
  svg.setAttribute("class", sample?.getAttribute("class") || "icon-xs");
  const style = sample?.getAttribute("style");
  if (style) svg.setAttribute("style", style);
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("width", sample?.getAttribute("width") || "16");
  svg.setAttribute("height", sample?.getAttribute("height") || "16");
  for (const attribute of Array.from(sample?.attributes ?? [])) {
    if (styleAttributes.has(attribute.name)) svg.setAttribute(attribute.name, attribute.value);
  }
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
  let styleAttributes = buttonStyleAttributes.get(btn);
  if (!styleAttributes) {
    styleAttributes = officialStyleAttributes(document);
    buttonStyleAttributes.set(btn, styleAttributes);
  }
  const next = createButtonIcon(source, name, sample, styleAttributes);
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
    syncTooltipPresentation();
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

function configureDockMenu(): void {
  if (window.__incodexPlatform !== "darwin" || window.__incodexDockMenuConfigured) return;
  const request = window.incodex?.requestIncognitoAction;
  if (!request) return;
  window.__incodexDockMenuConfigured = true;
  void request({
    action: "configure-dock-menu",
    label: t(isIncognitoWindow() ? "title" : "open"),
    requestId: newRequestId(),
  })
    .then((result) => {
      if (!result?.ok) window.__incodexDockMenuConfigured = false;
    })
    .catch(() => {
      window.__incodexDockMenuConfigured = false;
    });
}

function configureStatusMenu(): void {
  if (window.__incodexPlatform !== "darwin" || window.__incodexStatusMenuConfigured) return;
  const request = window.incodex?.requestIncognitoAction;
  if (!request) return;
  window.__incodexStatusMenuConfigured = true;
  void request({
    action: "configure-status-menu",
    label: t(isIncognitoWindow() ? "title" : "open"),
    requestId: newRequestId(),
  })
    .then((result) => {
      if (!result?.ok) window.__incodexStatusMenuConfigured = false;
    })
    .catch(() => {
      window.__incodexStatusMenuConfigured = false;
    });
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
  `;
}

function hideLaunchError(): void {
  notifications.hideError();
}

function showLaunchError(): void {
  notifications.showError({
    title: t("errorTitle"),
    body: t("errorBody"),
    retryLabel: t("errorRetry"),
    onRetry: () => { void activate(); },
  });
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
    syncTooltipPresentation() &&
    (btn.getAttribute("data-incodex-hovered") === "true" || document.activeElement === btn) &&
    !(search && searchTooltipOpen(search))
  );
}

function landingStillMounted(): boolean {
  const slot = isIncognitoWindow() && !bannerDismissed() ? findOfficialBannerSlot() : null;
  return !notifications.bannerNeedsReconcile(slot);
}

function tooltipMountStillPresent(): boolean {
  const host = document.querySelector<HTMLElement>(`[${TIP_HOST_ATTR}]`);
  const tip = host?.querySelector<HTMLElement>(`[${TIP_ATTR}]`);
  return Boolean(host?.isConnected && tip?.isConnected && tip.parentElement === host);
}

function needsInject(): boolean {
  return (
    tooltipState.renderer?.needsRemount() ||
    !buttonStillBesideSearch() ||
    !tooltipMountStillPresent() ||
    !landingStillMounted() ||
    notifications.errorNeedsMount() ||
    profileMaskNeedsInject()
  );
}

function buildButton(search: HTMLElement): HTMLElement {
  disposeActiveTooltip();
  const styleAttributes = officialStyleAttributes(document);
  const btn = search.cloneNode(false) as HTMLElement;
  buttonStyleAttributes.set(btn, styleAttributes);
  for (const name of STRIP_CLONE_ATTRS) btn.removeAttribute(name);
  for (const name of [...btn.attributes].map((attr) => attr.name)) {
    if (name.startsWith("data-") && !styleAttributes.has(name)) btn.removeAttribute(name);
  }
  btn.setAttribute("type", "button");
  btn.setAttribute(BTN_ATTR, "true");
  btn.setAttribute("data-incodex-hovered", "false");
  btn.className = search.className;
  const sample = search.querySelector<SVGElement>("svg");
  const svg = createButtonIcon(ICON_SVG, "hat-glasses", sample, styleAttributes);
  if (svg) btn.append(cloneButtonIconLayout(svg, sample, search, styleAttributes));
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
  tooltipState.lifecycle = tooltipLifecycle;
  btn.addEventListener(
    "click",
    (event) => {
      event.preventDefault();
      event.stopImmediatePropagation();
      setButtonHover(btn, false);
      tooltipLifecycle.trigger();
      void activate().then((completed) => {
        if (completed && btn.isConnected) btn.blur();
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
  // Presentation is supplied by the linked official Search tooltip. Until
  // sampled, keep the accessible label without a separate native tooltip.
  const text = document.createElement("div");
  text.className = "flex items-center gap-2";
  const label = document.createElement("div");
  label.className = "min-w-0";
  label.setAttribute("data-incodex-tooltip-label", "true");
  const kbd = document.createElement("kbd");
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

let tooltipObservedSearch: HTMLElement | null = null;
let tooltipObservedElement: HTMLElement | null = null;

function observeOfficialTooltip(search: HTMLElement | null): void {
  const element = findOfficialTooltipElement(search);
  if (search === tooltipObservedSearch && element === tooltipObservedElement) return;
  tooltipObservedSearch = search;
  tooltipObservedElement = element;
  window.__incodexTooltipPresentationObserver?.disconnect();
  const observer = new MutationObserver(() => syncTooltipPresentation());
  window.__incodexTooltipPresentationObserver = observer;
  if (search) {
    observer.observe(search, { attributes: true, attributeFilter: ["aria-describedby"] });
    if (search.parentElement) {
      observer.observe(search.parentElement, { attributes: true, attributeFilter: ["aria-describedby"] });
    }
  }
  if (element) observer.observe(element, { attributes: true, subtree: true, attributeFilter: ["class", "data-side"] });
}

function syncTooltipPresentation(): boolean {
  if (tooltipState.renderer?.ready()) {
    document.querySelector<HTMLElement>(`[${BTN_ATTR}]`)?.removeAttribute("title");
    return true;
  }
  const search = findSearchButton();
  observeOfficialTooltip(search);
  const sample = officialTooltipPresentation.read(search);
  const btn = document.querySelector<HTMLElement>(`[${BTN_ATTR}]`);
  // Also remove a fallback left by an older injector on an existing button.
  btn?.removeAttribute("title");
  const tip = document.querySelector<HTMLElement>(`[${TIP_ATTR}]`);
  if (!sample) {
    hideTooltip();
    return false;
  }
  if (tip && tip.className !== sample.className) tip.className = sample.className;
  const kbd = tip?.querySelector("kbd");
  if (kbd && kbd.className !== sample.shortcutClassName) kbd.className = sample.shortcutClassName;
  return true;
}

function tooltipEl(): HTMLElement {
  return ensureTooltipMount();
}

function showTooltip(btn: HTMLElement): void {
  if (tooltipState.renderer?.ready()) {
    tooltipState.renderer.show(btn, labelFor(isIncognitoWindow()), shortcutLabel(), findSearchButton());
    return;
  }
  const tip = tooltipEl();
  if (!syncTooltipPresentation()) return;
  const sample = officialTooltipPresentation.read(findSearchButton());
  if (!sample?.side || sample.gap === undefined) {
    hideTooltip();
    return;
  }
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
  if (sample.side === "bottom") {
    host.style.top = `${Math.max(8, rect.bottom + sample.gap)}px`;
    host.style.bottom = "auto";
  } else {
    host.style.top = "auto";
    host.style.bottom = `${Math.max(8, window.innerHeight - rect.top + sample.gap)}px`;
  }
  host.style.visibility = "";
}

function hideTooltip(): void {
  tooltipState.renderer?.hide();
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
      document.querySelector(`[${BANNER_HOST_ATTR}]`)?.querySelector(`[${BANNER_TITLE_ATTR}]`),
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
  ensureLanding();
  refreshUiProbe();
}

function classNameOf(element: Element): string {
  return element.getAttribute("class") ?? "";
}

function findOfficialBannerSlot(): HTMLElement | null {
  const candidates = [...document.querySelectorAll<HTMLElement>("div")].filter((el) => {
    if (el.hasAttribute(BANNER_HOST_ATTR)) return false;
    const classes = classNameOf(el).split(/\s+/);
    return classes.includes("home-banners") || (
      classes.includes("not-has-[>:not([hidden])]:hidden") &&
      classes.some((name) => name.includes("has-[[data-home-beacon-banner]]:mx-0"))
    );
  });
  return candidates.length === 1 ? candidates[0]! : null;
}

function ensureLaunchError(): void {
  // Warning refresh is independent of the home-only privacy notice.
  ensureLanding();
}

function ensureLanding(): void {
  const slot = isIncognitoWindow() && !bannerDismissed() ? findOfficialBannerSlot() : null;
  const copy = slot ? {
    title: t("title"),
    body: t("body"),
    closeLabel: t("dismiss"),
    icon: ICON_SVG,
    onClose: dismissBanner,
  } : null;
  void notifications.ensure(slot, copy).catch((error) =>
    console.warn("[incodex] official privacy banner unavailable", String(error)),
  );
}

function ensureButton(): void {
  let btn = document.querySelector<HTMLElement>(`[${BTN_ATTR}]`);
  const search = findSearchButton();
  const placement = search ? searchButtonPlacement(search) : null;
  if (!search || !placement) {
    window.__incodexSearchAppearanceObserver?.disconnect();
    window.__incodexSearchAppearanceObserver = undefined;
    window.__incodexObservedSearch = undefined;
    window.__incodexObservedButton = undefined;
    if (btn?.isConnected) dismissActiveTooltip();
    else disposeActiveTooltip();
    return;
  }

  if (!btn) btn = buildButton(search);
  if (!isParkedLeftOfSearch(btn, search)) {
    placement.parent.insertBefore(btn, placement.before);
  }
  observeSearchAppearance(search, btn);
  apply();
  ensureTooltipMount();
  syncTooltipPresentation();
  if (tooltipState.renderer?.needsRemount()) {
    tooltipState.renderer.dispose();
    tooltipState.renderer = null;
  }
  if (!tooltipState.renderer) {
    tooltipState.renderer = createOfficialTooltipRenderer(document, () => loadOfficialTooltipModules(document, readOfficialSource));
  }
  const renderer = tooltipState.renderer;
  if (renderer.needsPreparation()) {
    void renderer.prepare().then(() => {
      if (tooltipState.renderer !== renderer || !btn?.isConnected) return;
      // Async readiness must not reconstruct canceled input from stale DOM state.
      tooltipState.lifecycle?.presentationReady();
    }).catch((error) => console.warn("[incodex] official tooltip renderer unavailable", String(error)));
  }
  // 搜索就绪即可准备本窗口官方组件，不必等首页横幅槽挂载。
  if (isIncognitoWindow() && !bannerDismissed() && notifications.needsPreparation()) {
    void notifications.prepare().catch((error) =>
      console.warn("[incodex] official privacy banner unavailable", String(error)),
    );
  }
}

function observeSearchAppearance(search: HTMLElement, button: HTMLElement): void {
  if (window.__incodexObservedSearch === search && window.__incodexObservedButton === button &&
      buttonStyleAttributes.has(button)) return;
  window.__incodexSearchAppearanceObserver?.disconnect();
  const styleAttributes = buttonStyleAttributes.get(button) ?? officialStyleAttributes(document);
  buttonStyleAttributes.set(button, styleAttributes);
  syncOfficialButtonAppearance(search, button, styleAttributes);
  const observer = new MutationObserver(() => {
    if (search.isConnected && button.isConnected) {
      syncOfficialButtonAppearance(search, button, styleAttributes);
    }
  });
  observer.observe(search, {
    attributes: true,
    attributeFilter: ["class", "style", ...styleAttributes],
  });
  window.__incodexSearchAppearanceObserver = observer;
  window.__incodexObservedSearch = search;
  window.__incodexObservedButton = button;
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
    // Background Electron windows can suspend animation frames. Notification
    // teardown and native readiness must still follow actual DOM mutations.
    ensureLanding();
    refreshUiProbe();
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(function injectOnAnimationFrame(): void {
      scheduled = false;
      syncTooltipPresentation();
      refreshUiProbe();
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
  configureDockMenu();
  configureStatusMenu();
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
  window.addEventListener("blur", () => tooltipState.lifecycle?.windowBlur());
  window.addEventListener("focus", () => tooltipState.lifecycle?.windowFocus());
  window.addEventListener(TOOLTIP_DISMISS_EVENT, () => tooltipState.lifecycle?.dismiss());
  ensureMutationObserver();
}

declare global {
  interface Window {
    __incodexNotifications?: ReturnType<typeof createOfficialNotifications>;
    __incodexTooltipState?: ReturnType<typeof sharedTooltipState>;
    __incodexStarted?: boolean;
    __incodexIncognito?: boolean;
    __incodexDockMenuConfigured?: boolean;
    __incodexStatusMenuConfigured?: boolean;
    __incodexLocale?: string;
    __incodexPlatform?: string;
    __incodexMutationObserver?: MutationObserver;
    __incodexTooltipPresentationObserver?: MutationObserver;
    __incodexProfileObservationEnabled?: boolean;
    __incodexSearchAppearanceObserver?: MutationObserver;
    __incodexObservedSearch?: HTMLElement;
    __incodexObservedButton?: HTMLElement;
    __incodexProfileMaskHealth?: boolean;
    __incodexRefreshProfileMaskHealth?: () => boolean;
    __incodexUiProbe?: ReturnType<typeof deriveUiProbe>;
    incodex?: {
      requestIncognitoAction?: (payload: {
        action: IncognitoBridgeAction;
        label?: string;
        requestId: string;
      }) => Promise<IncognitoActionResponse>;
    };
  }
}

window.__incodexRefreshProfileMaskHealth = refreshProfileMaskHealth;

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", start, { once: true });
} else {
  start();
}
