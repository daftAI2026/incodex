const STYLE_ID = "incodex-privacy-style";
const BTN_ATTR = "data-incodex-privacy-toggle";
const TIP_ATTR = "data-incodex-tooltip";
const ROOT_ATTR = "data-incodex-privacy";
const STORAGE_KEY = "incodex-privacy";
const CLUSTER_ATTR = "data-incodex-header-cluster";
const SHORTCUT_LABEL = "⇧⌘N";

// Private existing state: chat titles and folder names.
// Product chrome stays: 新对话, 项目 heading, add-project.
const HIDE_SELECTORS = [
  "[data-app-action-sidebar-thread-row]",
  "[data-app-action-sidebar-project-row]",
];
const HISTORY_SECTIONS = new Set(["Pinned", "Recents"]);
const SHOW_MORE_LABELS = new Set(["Show more", "显示更多"]);

const ICON_SVG = `{{HAT_GLASSES_SVG}}`;
const SEARCH_LABELS = new Set(["Search", "搜索"]);

function readEnabled(): boolean {
  try {
    return window.localStorage.getItem(STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

function writeEnabled(on: boolean): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, on ? "1" : "0");
  } catch {
    /* ignore */
  }
}

function labelFor(on: boolean): string {
  return on ? "退出无痕" : "无痕";
}

function apply(on: boolean): void {
  document.documentElement.setAttribute(ROOT_ATTR, on ? "on" : "off");
  const btn = document.querySelector<HTMLElement>(`[${BTN_ATTR}]`);
  if (btn) {
    btn.setAttribute("aria-pressed", on ? "true" : "false");
    btn.setAttribute("aria-label", labelFor(on));
  }
  const label = document.querySelector<HTMLElement>("[data-incodex-tooltip-label]");
  if (label) label.textContent = labelFor(on);
  syncDerivedChrome(on);
}

function syncDerivedChrome(on: boolean): void {
  for (const section of document.querySelectorAll<HTMLElement>("[data-app-action-sidebar-section]")) {
    const heading = section.getAttribute("data-app-action-sidebar-section-heading") || "";
    if (on && HISTORY_SECTIONS.has(heading)) section.setAttribute("data-incodex-empty-section", "");
    else section.removeAttribute("data-incodex-empty-section");

    for (const btn of section.querySelectorAll<HTMLElement>("button")) {
      const text = (btn.textContent || "").replace(/\s+/g, " ").trim();
      if (!SHOW_MORE_LABELS.has(text)) continue;
      if (on) btn.setAttribute("data-incodex-show-more", "");
      else btn.removeAttribute("data-incodex-show-more");
    }
  }
}

function toggle(): void {
  const next = document.documentElement.getAttribute(ROOT_ATTR) !== "on";
  writeEnabled(next);
  apply(next);
  hideTooltip();
}

function ensureStyle(): void {
  let style = document.getElementById(STYLE_ID) as HTMLStyleElement | null;
  if (!style) {
    style = document.createElement("style");
    style.id = STYLE_ID;
    document.head.append(style);
  }
  const hide = HIDE_SELECTORS.map((sel) => `html[${ROOT_ATTR}="on"] ${sel}`).join(",\n");
  style.textContent = `
    ${hide} { display: none !important; }
    html[${ROOT_ATTR}="on"] [data-incodex-empty-section],
    html[${ROOT_ATTR}="on"] [data-app-action-sidebar-project-show-all-toggle],
    html[${ROOT_ATTR}="on"] [data-incodex-show-more] {
      display: none !important;
    }
    [${TIP_ATTR}] {
      position: fixed;
      z-index: 50;
      display: none;
      width: max-content;
      max-width: 240px;
      pointer-events: none !important;
      user-select: none;
      box-sizing: border-box;
    }
    [${TIP_ATTR}][data-open="true"] { display: block; }
  `;
}

function findSearchButton(): HTMLElement | null {
  return (
    [...document.querySelectorAll<HTMLElement>("button")].find((btn) =>
      SEARCH_LABELS.has((btn.getAttribute("aria-label") || "").trim()),
    ) ?? null
  );
}

function findHeaderCluster(search: HTMLElement): HTMLElement | null {
  return search.closest<HTMLElement>(".ms-auto.flex.items-center");
}

function isParkedInCluster(btn: HTMLElement, cluster: HTMLElement, search: HTMLElement): boolean {
  return btn.parentElement === cluster && !search.contains(btn) && !btn.contains(search);
}

function buildButton(search: HTMLElement): HTMLElement {
  const btn = search.cloneNode(false) as HTMLElement;
  btn.removeAttribute("id");
  btn.removeAttribute("aria-haspopup");
  btn.removeAttribute("aria-expanded");
  btn.removeAttribute("data-state");
  btn.setAttribute("type", "button");
  btn.setAttribute(BTN_ATTR, "true");
  btn.className = search.className;
  const wrap = document.createElement("span");
  wrap.innerHTML = ICON_SVG.trim();
  const svg = wrap.firstElementChild as SVGElement | null;
  if (svg) {
    const sample = search.querySelector("svg");
    svg.setAttribute("class", sample?.getAttribute("class") || "icon-xs");
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("width", sample?.getAttribute("width") || "16");
    svg.setAttribute("height", sample?.getAttribute("height") || "16");
    btn.append(svg);
  }
  btn.addEventListener(
    "click",
    (event) => {
      event.preventDefault();
      event.stopImmediatePropagation();
      toggle();
    },
    true,
  );
  btn.addEventListener("pointerenter", () => showTooltip(btn));
  btn.addEventListener("pointerleave", hideTooltip);
  btn.addEventListener("focus", () => showTooltip(btn));
  btn.addEventListener("blur", hideTooltip);
  return btn;
}

function tooltipEl(): HTMLElement {
  let tip = document.querySelector<HTMLElement>(`[${TIP_ATTR}]`);
  if (tip) return tip;
  tip = document.createElement("div");
  tip.setAttribute(TIP_ATTR, "true");
  tip.setAttribute("role", "tooltip");
  tip.className =
    "z-50 w-fit select-none text-sm whitespace-normal break-words rounded-lg border border-default bg-surface-elevated-secondary text-default px-2 py-1.5";
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

function showTooltip(btn: HTMLElement): void {
  const tip = tooltipEl();
  const label = tip.querySelector<HTMLElement>("[data-incodex-tooltip-label]");
  if (label) label.textContent = labelFor(btn.getAttribute("aria-pressed") === "true");
  tip.setAttribute("data-open", "true");
  const rect = btn.getBoundingClientRect();
  const tipRect = tip.getBoundingClientRect();
  const left = Math.min(
    window.innerWidth - tipRect.width - 8,
    Math.max(8, rect.left + rect.width / 2 - tipRect.width / 2),
  );
  const top = Math.max(8, rect.top - tipRect.height - 8);
  tip.style.left = `${left}px`;
  tip.style.top = `${top}px`;
}

function hideTooltip(): void {
  const tip = document.querySelector<HTMLElement>(`[${TIP_ATTR}]`);
  if (tip) tip.removeAttribute("data-open");
}

function ensureButton(): void {
  const search = findSearchButton();
  if (!search) return;
  const cluster = findHeaderCluster(search);
  if (!cluster) return;
  cluster.setAttribute(CLUSTER_ATTR, "true");

  let btn = document.querySelector<HTMLElement>(`[${BTN_ATTR}]`);
  if (!btn) btn = buildButton(search);
  if (!isParkedInCluster(btn, cluster, search)) {
    cluster.insertBefore(btn, cluster.firstElementChild);
  }
  apply(readEnabled());
}

function onHotkey(event: KeyboardEvent): void {
  if (!(event.metaKey || event.ctrlKey) || !event.shiftKey) return;
  if (event.code !== "KeyN" && event.key.toLowerCase() !== "n") return;
  event.preventDefault();
  event.stopImmediatePropagation();
  toggle();
}

function start(): void {
  if (window.__incodexStarted) return;
  window.__incodexStarted = true;
  ensureStyle();
  ensureButton();
  apply(readEnabled());
  window.addEventListener("keydown", onHotkey, true);
  let scheduled = false;
  const observer = new MutationObserver(() => {
    if (scheduled) return;
    scheduled = true;
    requestAnimationFrame(() => {
      scheduled = false;
      ensureButton();
    });
  });
  observer.observe(document.documentElement, { childList: true, subtree: true });
}

declare global {
  interface Window {
    __incodexStarted?: boolean;
  }
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", start, { once: true });
} else {
  start();
}
