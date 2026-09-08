/**
 * [INPUT]: 依赖 model.ts 的编辑状态、copy.ts 的本地化文案、presets.ts 的背景分层与 icons.ts 的图标
 * [OUTPUT]: 对外提供稳定 DOM 模板，以及重建时保留检查器滚动和焦点的视图记忆工具；padding 刻度仅作装饰
 * [POS]: capture-window 的声明式视图边界，只表达产品语义和可访问结构，不持有交互状态
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import type { CaptureWindowCopy } from "./copy.ts";
import { escapeAttribute } from "./color-popover.ts";
import { captureIcon } from "./icons.ts";
import {
  CAPTURE_MAX_PADDING,
  CAPTURE_MIN_PADDING,
  CAPTURE_PADDING_STEP,
  CAPTURE_MAX_ZOOM,
  CAPTURE_MIN_ZOOM,
  type CaptureWindowState,
} from "./model.ts";
import {
  SYSTEM_WALLPAPER_CURRENT_ID,
  type SystemWallpaperState,
} from "./system-wallpapers.ts";
import {
  type CapturePresetSection,
  captureBackgroundSection,
  capturePlainColors,
  capturePresetSection,
  capturePresetSwatch,
  isCapturePlainColor,
} from "./presets.ts";

export type CaptureWindowViewOptions = {
  lastBackgroundColor?: string;
  systemWallpapers?: SystemWallpaperState;
  wallpaperDataUrl?: string | null;
};

export type CaptureWindowRenderMemory = {
  action?: string;
  inspectorScrollTop: number;
  wallpaper?: string;
};

export function rememberCaptureWindowRender(root: HTMLElement): CaptureWindowRenderMemory {
  const inspector = root.querySelector<HTMLElement>(".incodex-capture-inspector-scroll");
  const active = document.activeElement instanceof HTMLElement && root.contains(document.activeElement)
    ? document.activeElement
    : null;
  return {
    action: active?.dataset.action,
    inspectorScrollTop: inspector?.scrollTop ?? 0,
    wallpaper: active?.dataset.systemWallpaper,
  };
}

export function restoreCaptureWindowRender(
  root: HTMLElement,
  memory: CaptureWindowRenderMemory,
): void {
  const inspector = root.querySelector<HTMLElement>(".incodex-capture-inspector-scroll");
  if (inspector) inspector.scrollTop = memory.inspectorScrollTop;
  if (memory.wallpaper) {
    [...root.querySelectorAll<HTMLElement>("[data-system-wallpaper]")]
      .find((option) => option.dataset.systemWallpaper === memory.wallpaper)
      ?.focus();
  } else if (memory.action) {
    root.querySelector<HTMLElement>(`[data-action='${memory.action}']`)?.focus();
  }
}

export function captureWindowTemplate(
  state: CaptureWindowState,
  copy: CaptureWindowCopy,
  options: CaptureWindowViewOptions = {},
): string {
  const lastBackgroundColor = options.lastBackgroundColor ?? "#2B3440";
  const wallpaperDataUrl = options.wallpaperDataUrl ?? (
    state.background.kind === "wallpaper" && !state.background.systemId
      ? state.background.dataUrl
      : null
  );
  const systemWallpapers = options.systemWallpapers ?? { entries: [], status: "unavailable" as const };
  return `
    <div class="incodex-capture-backdrop" aria-hidden="true"></div>
    <section class="incodex-capture-dialog" role="dialog" aria-modal="true" aria-labelledby="incodex-capture-title">
      <header class="incodex-capture-header">
        <div class="incodex-capture-heading">
          ${captureIcon("camera")}
          <h1 class="incodex-capture-title" id="incodex-capture-title">${copy.title}</h1>
        </div>
        ${iconButton("close", "x", copy.close)}
      </header>
      <div class="incodex-capture-workspace">
        <section class="incodex-capture-preview-pane" aria-label="${copy.preview}">
          ${captureToolbarTemplate(state, copy)}
          <div class="incodex-capture-stage" data-tool="${state.tool}">
            <div class="incodex-capture-canvas-frame">
              <div class="incodex-capture-region-layer" aria-hidden="true"></div>
            </div>
          </div>
        </section>
        ${inspectorTemplate(state, copy, lastBackgroundColor, wallpaperDataUrl, systemWallpapers)}
      </div>
      ${footerTemplate(copy)}
    </section>
  `;
}

export function captureToolbarTemplate(
  state: CaptureWindowState,
  copy: CaptureWindowCopy,
): string {
  const hint = captureRegionHint(state, copy);
  return `
    <div class="incodex-capture-toolbar">
      <span class="incodex-capture-region-hint">${hint}</span>
      <div class="incodex-capture-toolbar-controls" role="group" aria-label="${copy.tools}">
        ${iconButton("tool-move", "hand", copy.move, false, state.tool === "move")}
        ${iconButton("tool-redact", "square-dashed", copy.redact, false, state.tool === "redact")}
        ${state.tool === "redact" ? redactControlsTemplate(state, copy) : ""}
        ${toolbarDivider()}
        ${iconButton("undo", "undo", copy.undo, state.history.past.length === 0)}
        ${iconButton("redo", "redo", copy.redo, state.history.future.length === 0)}
        ${toolbarDivider()}
        ${iconButton("zoom-out", "zoom-out", copy.zoomOut, state.zoom <= CAPTURE_MIN_ZOOM)}
        <button class="incodex-capture-zoom-reset" data-action="zoom-reset" type="button" title="${copy.zoomReset}">${Math.round(state.zoom * 100)}%</button>
        ${iconButton("zoom-in", "zoom-in", copy.zoomIn, state.zoom >= CAPTURE_MAX_ZOOM)}
        ${iconButton("zoom-fit", "maximize", copy.zoomReset)}
      </div>
    </div>
  `;
}

function captureRegionHint(state: CaptureWindowState, copy: CaptureWindowCopy): string {
  if (state.tool !== "redact") return "";
  return state.redactionSource === "auto" ? copy.regionHint : copy.regionHintDraw;
}

function redactControlsTemplate(state: CaptureWindowState, copy: CaptureWindowCopy): string {
  return `
    ${toolbarDivider()}
    ${iconButton("source-auto", "scan-search", copy.sourceAuto, false, state.redactionSource === "auto", undefined, copy.sourceAutoHint)}
    ${iconButton("source-draw", "pen-line", copy.sourceDraw, false, state.redactionSource === "draw", undefined, copy.sourceDrawHint)}
    ${toolbarDivider()}
    ${iconButton("style-mosaic", "grid-3x3", copy.mosaic, false, state.redactionStyle === "mosaic", "mosaic")}
    ${iconButton("style-blur", "droplet", copy.blur, false, state.redactionStyle === "blur", "blur")}
    ${iconButton("style-solid", "square", copy.solid, false, state.redactionStyle === "solid", "solid")}
    ${solidColorTemplate(state, copy)}
  `;
}

function toolbarDivider(): string {
  return '<span class="incodex-capture-toolbar-divider" aria-hidden="true"></span>';
}

function inspectorTemplate(
  state: CaptureWindowState,
  copy: CaptureWindowCopy,
  lastBackgroundColor: string,
  wallpaperDataUrl: string | null,
  systemWallpapers: SystemWallpaperState,
): string {
  return `
    <aside class="incodex-capture-inspector">
      <h2 class="incodex-capture-section-title" id="incodex-capture-background-title">${copy.background}</h2>
      <div class="incodex-capture-inspector-scroll" tabindex="0" role="region" aria-labelledby="incodex-capture-background-title">
      <div class="incodex-capture-inspector-content">
      <section class="incodex-capture-section">
        ${backgroundGridTemplate(state, copy, lastBackgroundColor, wallpaperDataUrl, systemWallpapers)}
      </section>
      <section class="incodex-capture-section">
        <div class="incodex-capture-row incodex-capture-padding-heading">
          <h2 class="incodex-capture-section-title">${copy.padding}</h2>
          <span class="incodex-capture-value" data-value="padding">${state.padding}%</span>
        </div>
        <div class="incodex-capture-range-field">
          <input class="incodex-capture-range" data-input="padding" aria-label="${copy.padding}" type="range" min="${CAPTURE_MIN_PADDING}" max="${CAPTURE_MAX_PADDING}" step="${CAPTURE_PADDING_STEP}" value="${state.padding}">
          <div class="incodex-capture-range-ticks" aria-hidden="true">
            ${Array.from({ length: 11 }, (_, index) => `<i style="--capture-tick-position: ${index * 10}%"></i>`).join("")}
          </div>
        </div>
      </section>
      <section class="incodex-capture-section incodex-capture-row">
        <h2 class="incodex-capture-section-title">${copy.backgroundNone}</h2>
        <input class="incodex-capture-switch" data-input="none" type="checkbox" aria-label="${copy.backgroundNone}" ${checked(state.background.kind === "transparent")}>
      </section>
      <section class="incodex-capture-section incodex-capture-row">
        <h2 class="incodex-capture-section-title">${copy.shadow}</h2>
        <input class="incodex-capture-switch" data-input="shadow" type="checkbox" aria-label="${copy.shadow}" ${checked(state.shadow)}>
      </section>
      <section class="incodex-capture-section incodex-capture-row">
        <div class="incodex-capture-privacy-copy">
          <h2 class="incodex-capture-section-title">${copy.privacy}</h2>
          <p class="incodex-capture-section-description">${copy.privacyDescription}</p>
        </div>
        <input class="incodex-capture-switch" data-input="privacy" type="checkbox" aria-label="${copy.privacy}" ${checked(state.privacyEnabled)}>
      </section>
      </div>
      </div>
    </aside>
  `;
}

function solidColorTemplate(state: CaptureWindowState, copy: CaptureWindowCopy): string {
  return `
    <button class="incodex-capture-solid-color" data-color-trigger="solid" type="button" aria-label="${copy.maskColor}" title="${copy.maskColor}" aria-haspopup="dialog" aria-expanded="false" data-state="closed" ${state.redactionStyle === "solid" ? "" : "hidden"}>
      <span style="--capture-solid-color:${state.solidColor}"></span>
    </button>
  `;
}

function backgroundGridTemplate(
  state: CaptureWindowState,
  copy: CaptureWindowCopy,
  lastBackgroundColor: string,
  wallpaperDataUrl: string | null,
  systemWallpapers: SystemWallpaperState,
): string {
  const custom = state.background.kind === "color" && !isCapturePlainColor(state.background.color);
  const wallpaper = state.background.kind === "wallpaper" && !state.background.systemId;
  const customIcon = `<span data-background-custom-icon ${custom ? "hidden" : ""}>${captureIcon("pipette")}</span>`;
  const wallpaperImage = wallpaperDataUrl ?? "";
  const changeImageHidden = wallpaper && wallpaperDataUrl ? "" : " hidden";
  const gradients = capturePresetSection("gradients");
  const wallpapers = capturePresetSection("wallpapers");
  const activeSection = captureBackgroundSection(state.background);
  const wallpapersActive = activeSection === "wallpapers" || activeSection === "system-wallpapers";
  const visibleWallpapers = systemWallpapers.entries.filter((entry) => entry.id === SYSTEM_WALLPAPER_CURRENT_ID || entry.loadStatus !== undefined);
  const currentWallpaper = visibleWallpapers.length > 0 && visibleWallpapers.every((entry) =>
    entry.loadStatus === undefined || entry.loadStatus === "ready" || entry.loadStatus === "loading");
  const currentWallpaperLoading = systemWallpapers.status === "loading";
  const currentWallpaperDisabled = currentWallpaperLoading || systemWallpapers.status === "unavailable";
  const currentWallpaperSelected = state.background.kind === "wallpaper" &&
    state.background.systemId === SYSTEM_WALLPAPER_CURRENT_ID;
  return `
    <div class="incodex-capture-background-sections">
      ${presetSectionTemplate(gradients, copy.backgroundGradients, state, activeSection, copy)}
      <section class="incodex-capture-background-section" data-background-section="wallpapers" data-active="${wallpapersActive}" aria-label="${copy.backgroundWallpapers}">
        <div class="incodex-capture-background-section-heading">
          <h3 class="incodex-capture-background-section-title">${copy.backgroundWallpapers}</h3>
          ${currentWallpaper ? "" : `<button class="incodex-capture-background-expand" data-action="load-current-wallpaper" title="${escapeAttribute(copy.wallpaperDownloadHint)}" data-selected="${currentWallpaperSelected}" type="button" aria-busy="${currentWallpaperLoading}" aria-pressed="${currentWallpaperSelected}"${currentWallpaperDisabled ? " disabled" : ""}>${copy.getCurrentWallpaper}</button>`}
        </div>
        <div class="incodex-capture-background-grid">
          ${presetButtonsTemplate(wallpapers, state)}
          ${visibleWallpapers.map((entry) => {
            const loading = entry.loadStatus === "loading";
            const label = entry.id === SYSTEM_WALLPAPER_CURRENT_ID ? copy.currentDesktop : entry.name;
            const hint = entry.loadStatus === "error" ? `${label} · ${copy.retryWallpaper}` : label;
            const selected = state.background.kind === "wallpaper" && state.background.systemId === entry.id;
            return `<button class="incodex-capture-background-option incodex-capture-wallpaper-label" data-system-wallpaper="${escapeAttribute(entry.id)}" data-load-status="${entry.loadStatus ?? "ready"}" type="button" data-selected="${selected}" aria-pressed="${selected}" aria-busy="${loading}" aria-label="${escapeAttribute(hint)}" title="${escapeAttribute(hint)}"${loading ? " disabled" : ""}>${loading ? '<span class="incodex-capture-skeleton" aria-hidden="true"></span>' : entry.loadStatus === "error" ? captureIcon("retake") : entry.thumbnail ? `<img src="${escapeAttribute(entry.thumbnail)}" alt="">` : captureIcon("download")}</button>`;
          }).join("")}
          <button class="incodex-capture-background-option incodex-capture-wallpaper-label" data-background-wallpaper type="button" data-selected="${wallpaper}" aria-label="${copy.wallpaper}" title="${copy.wallpaper}"><img data-wallpaper-preview src="${wallpaperImage}" alt="" ${wallpaperDataUrl ? "" : "hidden"}><span data-wallpaper-placeholder ${wallpaperDataUrl ? "hidden" : ""}>${captureIcon("plus")}</span></button>
        </div>
        <input class="incodex-capture-wallpaper-input" data-input="wallpaper" type="file" accept="image/png,image/jpeg,image/webp">
        <button class="incodex-capture-change-wallpaper" data-action="change-wallpaper" type="button"${changeImageHidden}>${copy.changeImage}</button>
        ${currentWallpaperStatusTemplate(systemWallpapers.status, copy)}
      </section>
      <section class="incodex-capture-background-section" data-background-section="plain-color" data-active="${activeSection === "plain-color"}" aria-label="${copy.backgroundPlainColor}">
        <h3 class="incodex-capture-background-section-title">${copy.backgroundPlainColor}</h3>
        <div class="incodex-capture-background-grid incodex-capture-background-grid-plain">
          ${plainColorButtonsTemplate(state, capturePlainColors)}
          <button class="incodex-capture-background-option incodex-capture-color-label" data-background-custom data-color-trigger="background" data-selected="${custom}" type="button" aria-label="${copy.custom}" title="${copy.custom}" aria-haspopup="dialog" aria-expanded="false" data-state="closed" style="--capture-swatch:${lastBackgroundColor}">${customIcon}</button>
        </div>
      </section>
    </div>
  `;
}

function currentWallpaperStatusTemplate(
  status: SystemWallpaperState["status"],
  copy: CaptureWindowCopy,
): string {
  if (status === "loading") {
    return `<p class="incodex-capture-section-description" data-current-wallpaper-status aria-live="polite">${copy.currentWallpaperLoading}</p>`;
  }
  if (status === "error") {
    return `<p class="incodex-capture-section-description" data-current-wallpaper-status role="alert">${copy.currentWallpaperError}</p>`;
  }
  if (status === "unavailable") {
    return `<p class="incodex-capture-section-description" data-current-wallpaper-status>${copy.currentWallpaperUnavailable}</p>`;
  }
  return "";
}

function plainColorButtonsTemplate(
  state: CaptureWindowState,
  colors: readonly string[],
): string {
  return colors.map((color) => {
    const selected = state.background.kind === "color" &&
      state.background.color.toLowerCase() === color.toLowerCase();
    return `<button class="incodex-capture-background-option" data-background-color="${color}" type="button" aria-label="${color}" title="${color}" aria-pressed="${selected}" style="--capture-swatch:${color}"></button>`;
  }).join("");
}

function presetSectionTemplate(
  section: CapturePresetSection,
  label: string,
  state: CaptureWindowState,
  activeSection: ReturnType<typeof captureBackgroundSection>,
  copy?: Pick<CaptureWindowCopy, "backgroundShowLess" | "backgroundShowMore">,
): string {
  const collapsible = section.id === "gradients" && copy;
  const heading = collapsible
    ? `<div class="incodex-capture-background-section-heading">
        <h3 class="incodex-capture-background-section-title">${label}</h3>
        <button class="incodex-capture-background-expand" data-action="toggle-gradients" type="button" aria-expanded="${state.gradientsExpanded}">${state.gradientsExpanded ? copy.backgroundShowLess : copy.backgroundShowMore}</button>
      </div>`
    : `<h3 class="incodex-capture-background-section-title">${label}</h3>`;
  return `
    <section class="incodex-capture-background-section" data-background-section="${section.id}" data-active="${activeSection === section.id}" data-expanded="${section.id === "gradients" && state.gradientsExpanded}" aria-label="${label}">
      ${heading}
      <div class="incodex-capture-background-grid">${presetButtonsTemplate(section, state)}</div>
    </section>
  `;
}

function presetButtonsTemplate(
  section: CapturePresetSection,
  state: CaptureWindowState,
): string {
  return section.presets.map((preset, index) => {
    const { id } = preset;
    const selected = state.background.kind === "preset" && state.background.id === id;
    const overflow = section.id === "gradients" && index >= 5;
    return `<button class="incodex-capture-background-option" data-background="${id}"${overflow ? " data-gradient-overflow" : ""} type="button" aria-label="${id}" title="${id}" aria-pressed="${selected}" style="--capture-swatch:${capturePresetSwatch(preset)}"${overflow && !state.gradientsExpanded ? " hidden" : ""}></button>`;
  }).join("");
}

function footerTemplate(copy: CaptureWindowCopy): string {
  return `
    <footer class="incodex-capture-footer">
      <button class="incodex-capture-button" data-action="retake" type="button">${captureIcon("retake")}<span>${copy.retake}</span></button>
      <span class="incodex-capture-footer-spacer"></span>
      <button class="incodex-capture-button incodex-capture-button-secondary" data-action="save" type="button">${captureIcon("save")}<span>${copy.save}</span></button>
      <button class="incodex-capture-button incodex-capture-button-primary" data-action="copy" type="button">${captureIcon("copy")}<span>${copy.copy}</span></button>
    </footer>
  `;
}

function iconButton(
  action: string,
  icon: Parameters<typeof captureIcon>[0],
  label: string,
  disabled = false,
  pressed?: boolean,
  redactionStyle?: string,
  title = label,
): string {
  const pressedAttribute = pressed === undefined ? "" : ` aria-pressed="${pressed}"`;
  const styleAttribute = redactionStyle ? ` data-redaction-style="${redactionStyle}"` : "";
  return `<button class="incodex-capture-icon-button" data-action="${action}"${styleAttribute} type="button" aria-label="${label}" title="${title}"${pressedAttribute} ${disabled ? "disabled" : ""}>${captureIcon(icon)}</button>`;
}

function checked(value: boolean): string {
  return value ? "checked" : "";
}
