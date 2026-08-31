import type { CaptureWindowCopy } from "./copy.ts";
import { captureIcon } from "./icons.ts";
import {
  CAPTURE_MAX_ZOOM,
  CAPTURE_MIN_ZOOM,
  type CaptureBackground,
  type CaptureWindowState,
} from "./model.ts";
import { capturePresets, capturePresetSwatch } from "./presets.ts";

export function captureWindowTemplate(
  state: CaptureWindowState,
  copy: CaptureWindowCopy,
): string {
  return `
    <div class="incodex-capture-backdrop" aria-hidden="true"></div>
    <section class="incodex-capture-dialog" role="dialog" aria-modal="true" aria-labelledby="incodex-capture-title">
      <header class="incodex-capture-header">
        <div class="incodex-capture-heading">
          <h1 class="incodex-capture-title" id="incodex-capture-title">${copy.title}</h1>
          <p class="incodex-capture-subtitle">${copy.subtitle}</p>
        </div>
        ${iconButton("close", "x", copy.close)}
      </header>
      <div class="incodex-capture-workspace">
        <section class="incodex-capture-preview-pane" aria-label="${copy.preview}">
          ${toolbarTemplate(state, copy)}
          <div class="incodex-capture-stage" data-tool="${state.tool}">
            <div class="incodex-capture-canvas-frame">
              <div class="incodex-capture-region-layer" aria-hidden="true"></div>
            </div>
          </div>
        </section>
        ${inspectorTemplate(state, copy)}
      </div>
      ${footerTemplate(copy)}
    </section>
  `;
}

function toolbarTemplate(state: CaptureWindowState, copy: CaptureWindowCopy): string {
  const hint = captureRegionHint(state, copy);
  return `
    <div class="incodex-capture-toolbar">
      <span class="incodex-capture-region-hint">${hint}</span>
      <div class="incodex-capture-toolbar-controls" role="group" aria-label="${copy.tools}">
        ${iconButton("tool-move", "move", copy.move, false, state.tool === "move")}
        ${iconButton("tool-redact", "scan", copy.redact, false, state.tool === "redact")}
        ${state.tool === "redact" ? redactControlsTemplate(state, copy) : ""}
        ${toolbarDivider()}
        ${iconButton("undo", "undo", copy.undo, state.history.past.length === 0)}
        ${iconButton("redo", "redo", copy.redo, state.history.future.length === 0)}
        ${toolbarDivider()}
        ${iconButton("zoom-out", "zoom-out", copy.zoomOut, state.zoom <= CAPTURE_MIN_ZOOM)}
        <button class="incodex-capture-zoom-reset" data-action="zoom-reset" type="button" title="100%">${Math.round(state.zoom * 100)}%</button>
        ${iconButton("zoom-in", "zoom-in", copy.zoomIn, state.zoom >= CAPTURE_MAX_ZOOM)}
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
    ${iconButton("source-auto", "scan-search", copy.sourceAutoHint, false, state.redactionSource === "auto")}
    ${iconButton("source-draw", "pen-line", copy.sourceDrawHint, false, state.redactionSource === "draw")}
    ${toolbarDivider()}
    ${iconButton("style-mosaic", "scan", copy.mosaic, false, state.redactionStyle === "mosaic", "mosaic")}
    ${iconButton("style-blur", "image", copy.blur, false, state.redactionStyle === "blur", "blur")}
    ${iconButton("style-solid", "palette", copy.solid, false, state.redactionStyle === "solid", "solid")}
  `;
}

function toolbarDivider(): string {
  return '<span class="incodex-capture-toolbar-divider" aria-hidden="true"></span>';
}

function inspectorTemplate(state: CaptureWindowState, copy: CaptureWindowCopy): string {
  return `
    <aside class="incodex-capture-inspector">
      <section class="incodex-capture-section">
        <div class="incodex-capture-section-heading">
          <h2 class="incodex-capture-section-title">${copy.privacy}</h2>
          <input class="incodex-capture-switch" data-input="privacy" type="checkbox" aria-label="${copy.privacy}" ${checked(state.privacyEnabled)}>
        </div>
        <p class="incodex-capture-section-description">${copy.privacyDescription}</p>
        <div class="incodex-capture-style-grid" role="group" aria-label="${copy.redactionStyle}">
          ${segmentButton("style-mosaic", "scan", copy.mosaic, state.redactionStyle === "mosaic", "mosaic")}
          ${segmentButton("style-blur", "image", copy.blur, state.redactionStyle === "blur", "blur")}
          ${segmentButton("style-solid", "palette", copy.solid, state.redactionStyle === "solid", "solid")}
        </div>
        ${solidColorTemplate(state, copy)}
      </section>
      <section class="incodex-capture-section">
        <div class="incodex-capture-section-heading">
          <h2 class="incodex-capture-section-title">${copy.layout}</h2>
        </div>
        <span class="incodex-capture-label">${copy.background}</span>
        ${backgroundGridTemplate(state, copy)}
        <div class="incodex-capture-custom-row">
          <label class="incodex-capture-color-label">
            ${captureIcon("palette", 16)}
            <span>${copy.custom}</span>
            <input class="incodex-capture-color-input" data-input="color" type="color" value="${backgroundColor(state.background)}">
          </label>
          <label class="incodex-capture-wallpaper-label">
            ${captureIcon("wallpaper", 16)}
            <span>${copy.wallpaper}</span>
            <input class="incodex-capture-wallpaper-input" data-input="wallpaper" type="file" accept="image/png,image/jpeg,image/webp">
          </label>
        </div>
        <div class="incodex-capture-control-stack">
          <label>
            <div class="incodex-capture-row">
              <span class="incodex-capture-label">${copy.padding}</span>
              <span class="incodex-capture-value" data-value="padding">${state.padding}px</span>
            </div>
            <input class="incodex-capture-range" data-input="padding" type="range" min="0" max="160" step="4" value="${state.padding}">
          </label>
          <label class="incodex-capture-row">
            <span class="incodex-capture-label">${copy.shadow}</span>
            <input class="incodex-capture-switch" data-input="shadow" type="checkbox" aria-label="${copy.shadow}" ${checked(state.shadow)}>
          </label>
        </div>
      </section>
    </aside>
  `;
}

function solidColorTemplate(state: CaptureWindowState, copy: CaptureWindowCopy): string {
  return `
    <label class="incodex-capture-solid-color-row" data-solid-color-row ${state.redactionStyle === "solid" ? "" : "hidden"}>
      <span class="incodex-capture-label">${copy.maskColor}</span>
      <input class="incodex-capture-color-input" data-input="solid-color" type="color" value="${state.solidColor}">
    </label>
  `;
}

function backgroundGridTemplate(
  state: CaptureWindowState,
  copy: CaptureWindowCopy,
): string {
  const presets = capturePresets.map((preset) => {
    const { id } = preset;
    const selected = state.background.kind === "preset" && state.background.id === id;
    return `<button class="incodex-capture-background-option" data-background="${id}" type="button" aria-label="${id}" aria-pressed="${selected}" style="--capture-swatch:${capturePresetSwatch(preset)}">${selected ? captureIcon("check", 14) : ""}</button>`;
  }).join("");
  const transparent = state.background.kind === "transparent";
  return `<div class="incodex-capture-background-grid">${presets}<button class="incodex-capture-background-option" data-background="transparent" type="button" aria-label="${copy.transparent}" aria-pressed="${transparent}" style="--capture-swatch:conic-gradient(#d9d9d9 25%,#fff 0 50%,#d9d9d9 0 75%,#fff 0) 0/12px 12px">${transparent ? captureIcon("check", 14) : ""}</button></div>`;
}

function footerTemplate(copy: CaptureWindowCopy): string {
  return `
    <footer class="incodex-capture-footer">
      <span class="incodex-capture-footer-note">${copy.manualReview}</span>
      <button class="incodex-capture-button" data-action="cancel" type="button">${copy.cancel}</button>
      <button class="incodex-capture-button incodex-capture-button-secondary" data-action="save" type="button">${captureIcon("download", 16)}<span>${copy.save}</span></button>
      <button class="incodex-capture-button incodex-capture-button-primary" data-action="copy" type="button">${captureIcon("copy", 16)}<span>${copy.copy}</span></button>
    </footer>
  `;
}

function segmentButton(
  action: string,
  icon: Parameters<typeof captureIcon>[0],
  label: string,
  pressed: boolean,
  redactionStyle?: string,
): string {
  const styleAttribute = redactionStyle ? ` data-redaction-style="${redactionStyle}"` : "";
  return `<button class="incodex-capture-segment" data-action="${action}"${styleAttribute} type="button" aria-pressed="${pressed}">${captureIcon(icon, 16)}<span class="incodex-capture-segment-label">${label}</span></button>`;
}

function iconButton(
  action: string,
  icon: Parameters<typeof captureIcon>[0],
  label: string,
  disabled = false,
  pressed?: boolean,
  redactionStyle?: string,
): string {
  const pressedAttribute = pressed === undefined ? "" : ` aria-pressed="${pressed}"`;
  const styleAttribute = redactionStyle ? ` data-redaction-style="${redactionStyle}"` : "";
  return `<button class="incodex-capture-icon-button" data-action="${action}"${styleAttribute} type="button" aria-label="${label}" title="${label}"${pressedAttribute} ${disabled ? "disabled" : ""}>${captureIcon(icon, 12)}</button>`;
}

function checked(value: boolean): string {
  return value ? "checked" : "";
}

function backgroundColor(background: CaptureBackground): string {
  return background.kind === "color" ? background.color : "#446f73";
}
