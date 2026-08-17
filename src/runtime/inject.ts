const STYLE_ID = "incodex-privacy-style";
const BTN_ATTR = "data-incodex-privacy-toggle";
const ROOT_ATTR = "data-incodex-privacy";
const STORAGE_KEY = "incodex-privacy";
const CLUSTER_ATTR = "data-incodex-header-cluster";

const HIDE_SELECTORS = [
  "[data-app-action-sidebar-thread-row]",
  "[data-app-action-sidebar-thread-id]",
  "[data-app-action-sidebar-thread-title]",
  "[data-app-action-sidebar-thread-pinned]",
  "[data-thread-title]",
];

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
  if (!btn) return;
  btn.setAttribute("aria-pressed", on ? "true" : "false");
  btn.setAttribute("aria-label", labelFor(on));
  btn.setAttribute("title", labelFor(on));
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
  const buttons = [...document.querySelectorAll<HTMLElement>("button")];
  return (
    buttons.find((btn) => SEARCH_LABELS.has((btn.getAttribute("aria-label") || "").trim())) ?? null
  );
}

function findHeaderCluster(search: HTMLElement): HTMLElement | null {
  const cluster = search.closest<HTMLElement>(".ms-auto.flex.items-center");
  return cluster;
}

function isParkedInCluster(btn: HTMLElement, cluster: HTMLElement, search: HTMLElement): boolean {
  if (btn.parentElement !== cluster) return false;
  if (search.closest(`[${BTN_ATTR}]`)) return false;
  return !search.contains(btn) && !btn.contains(search);
}

function headerButtonClass(search: HTMLElement): string {
  return search.className;
}

function buildButton(search: HTMLElement): HTMLElement {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = headerButtonClass(search);
  btn.setAttribute(BTN_ATTR, "true");
  const wrap = document.createElement("span");
  wrap.innerHTML = ICON_SVG.trim();
  const svg = wrap.firstElementChild as SVGElement | null;
  if (svg) {
    svg.setAttribute("class", "icon-xs");
    svg.setAttribute("aria-hidden", "true");
    svg.setAttribute("width", "16");
    svg.setAttribute("height", "16");
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
  return btn;
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

function start(): void {
  if (window.__incodexStarted) return;
  window.__incodexStarted = true;
  ensureStyle();
  ensureButton();
  apply(readEnabled());
  const observer = new MutationObserver(() => {
    ensureStyle();
    ensureButton();
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
