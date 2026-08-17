const STYLE_ID = "incodex-privacy-style";
const BTN_ATTR = "data-incodex-privacy-toggle";
const ROOT_ATTR = "data-incodex-privacy";
const STORAGE_KEY = "incodex-privacy";

const HIDE_SELECTORS = [
  "[data-app-action-sidebar-thread-row]",
  "[data-app-action-sidebar-thread-id]",
  "[data-app-action-sidebar-thread-title]",
  "[data-app-action-sidebar-thread-pinned]",
  "[data-thread-title]",
];

const ICON_SVG = `{{HAT_GLASSES_SVG}}`;

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

function apply(on: boolean): void {
  document.documentElement.setAttribute(ROOT_ATTR, on ? "on" : "off");
  const btn = document.querySelector<HTMLElement>(`[${BTN_ATTR}]`);
  if (btn) {
    btn.setAttribute("aria-pressed", on ? "true" : "false");
    btn.setAttribute("aria-label", on ? "Exit Incognito" : "Incognito");
    btn.setAttribute("title", on ? "Exit Incognito" : "Incognito");
  }
}

function toggle(): void {
  const next = document.documentElement.getAttribute(ROOT_ATTR) !== "on";
  writeEnabled(next);
  apply(next);
}

function ensureStyle(): void {
  if (document.getElementById(STYLE_ID)) return;
  const style = document.createElement("style");
  style.id = STYLE_ID;
  const hide = HIDE_SELECTORS.map((sel) => `html[${ROOT_ATTR}="on"] ${sel}`).join(",\n");
  style.textContent = `${hide} { display: none !important; }`;
  document.head.append(style);
}

function findSearchButton(): HTMLElement | null {
  const labeled = document.querySelector<HTMLElement>(
    'button[aria-label="Search"], button[aria-label="搜索"]',
  );
  if (labeled) return labeled;
  for (const btn of document.querySelectorAll<HTMLElement>("button")) {
    const label = (btn.getAttribute("aria-label") || btn.textContent || "").trim();
    if (label === "Search" || label === "搜索") return btn;
  }
  return null;
}

function buildButton(template: HTMLElement | null): HTMLElement {
  const btn = template ? (template.cloneNode(true) as HTMLElement) : document.createElement("button");
  btn.setAttribute(BTN_ATTR, "true");
  btn.setAttribute("type", "button");
  btn.removeAttribute("disabled");
  const svgHost = btn.querySelector("svg")?.parentElement ?? btn;
  const oldSvg = btn.querySelector("svg");
  const wrap = document.createElement("span");
  wrap.innerHTML = ICON_SVG.trim();
  const svg = wrap.firstElementChild as SVGElement | null;
  if (svg) {
    svg.setAttribute("class", "icon-xs");
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("width", "16");
    svg.setAttribute("height", "16");
    if (oldSvg) oldSvg.replaceWith(svg);
    else svgHost.append(svg);
  }
  btn.addEventListener(
    "click",
    (event) => {
      event.preventDefault();
      event.stopPropagation();
      toggle();
    },
    true,
  );
  return btn;
}

function ensureButton(): void {
  if (document.querySelector(`[${BTN_ATTR}]`)) return;
  const search = findSearchButton();
  const btn = buildButton(search);
  if (search?.parentElement) {
    search.parentElement.insertBefore(btn, search);
  } else {
    btn.style.position = "fixed";
    btn.style.top = "12px";
    btn.style.left = "12px";
    btn.style.zIndex = "2147483647";
    document.body?.append(btn);
  }
  apply(readEnabled());
}

function onHotkey(event: KeyboardEvent): void {
  if (!(event.metaKey || event.ctrlKey) || !event.shiftKey) return;
  if (event.key !== "." && event.code !== "Period") return;
  event.preventDefault();
  toggle();
}

function start(): void {
  if (window.__incodexStarted) return;
  window.__incodexStarted = true;
  ensureStyle();
  ensureButton();
  apply(readEnabled());
  window.addEventListener("keydown", onHotkey, true);
  const observer = new MutationObserver(() => {
    ensureStyle();
    ensureButton();
    apply(readEnabled());
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
