/**
 * [INPUT]: 依赖 model.ts 的背景命令与状态、color-popover.ts 的颜色同步、presets.ts 的分组语义
 * [OUTPUT]: 对外提供背景控件的事件绑定与稳定 DOM 状态同步
 * [POS]: capture-window 的背景交互边界，让 editor.ts 只负责编排编辑器生命周期
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { syncCaptureColorPopover } from "./color-popover.ts";
import type {
  CapturePresetId,
  CaptureWindowCommand,
  CaptureWindowState,
} from "./model.ts";
import { captureBackgroundSection, isCapturePlainColor } from "./presets.ts";

export type CaptureBackgroundActions = {
  dispatch: (command: CaptureWindowCommand) => void;
  pickWallpaper: () => void;
  readWallpaperDataUrl: () => string | null;
  setBackgroundColor: (color: string) => void;
};

export function wireCaptureBackgroundActions(
  root: HTMLElement,
  actions: CaptureBackgroundActions,
): void {
  for (const option of root.querySelectorAll<HTMLElement>("[data-background]")) {
    option.addEventListener("click", () => {
      const id = option.dataset.background;
      if (id === "transparent") {
        actions.dispatch({ kind: "set-background", background: { kind: "transparent" } });
        return;
      }
      actions.dispatch({
        kind: "set-background",
        background: { id: id as CapturePresetId, kind: "preset" },
      });
    });
  }
  for (const option of root.querySelectorAll<HTMLElement>("[data-background-color]")) {
    option.addEventListener("click", () => {
      const color = option.dataset.backgroundColor;
      if (color) actions.setBackgroundColor(color);
    });
  }
  const wallpaper = root.querySelector<HTMLButtonElement>("[data-background-wallpaper]");
  wallpaper?.addEventListener("click", () => {
    const dataUrl = actions.readWallpaperDataUrl();
    if (!dataUrl) {
      actions.pickWallpaper();
      return;
    }
    actions.dispatch({ background: { dataUrl, kind: "wallpaper" }, kind: "set-background" });
  });
  wallpaper?.addEventListener("dblclick", actions.pickWallpaper);
  root.querySelector<HTMLButtonElement>("[data-action='change-wallpaper']")?.addEventListener(
    "click",
    actions.pickWallpaper,
  );
}

export function syncCaptureBackgroundControls(
  root: HTMLElement,
  state: CaptureWindowState,
  lastBackgroundColor: string,
  wallpaperDataUrl: string | null,
): void {
  for (const option of root.querySelectorAll<HTMLElement>("[data-background]")) {
    const selected = option.dataset.background === "transparent"
      ? state.background.kind === "transparent"
      : state.background.kind === "preset" && option.dataset.background === state.background.id;
    option.setAttribute("aria-pressed", String(selected));
  }
  for (const option of root.querySelectorAll<HTMLElement>("[data-background-color]")) {
    const selected = state.background.kind === "color" &&
      option.dataset.backgroundColor?.toLowerCase() === state.background.color.toLowerCase();
    option.setAttribute("aria-pressed", String(selected));
  }

  const custom = root.querySelector<HTMLElement>("[data-background-custom]");
  if (custom) {
    const selected = state.background.kind === "color" && !isCapturePlainColor(state.background.color);
    custom.dataset.selected = String(selected);
    custom.querySelector<HTMLElement>("[data-background-custom-icon]")?.toggleAttribute(
      "hidden",
      selected,
    );
  }
  syncActiveSection(root, captureBackgroundSection(state.background));
  syncCaptureColorPopover(root, "background", lastBackgroundColor);
  syncWallpaperControls(root, state, wallpaperDataUrl);
}

function syncActiveSection(
  root: HTMLElement,
  activeSection: ReturnType<typeof captureBackgroundSection>,
): void {
  for (const section of root.querySelectorAll<HTMLElement>("[data-background-section]")) {
    section.dataset.active = String(
      section.dataset.backgroundSection === activeSection ||
        (activeSection === "none" && section.dataset.backgroundSection === "plain-color"),
    );
  }
}

function syncWallpaperControls(
  root: HTMLElement,
  state: CaptureWindowState,
  wallpaperDataUrl: string | null,
): void {
  const wallpaper = root.querySelector<HTMLElement>("[data-background-wallpaper]");
  if (wallpaper) wallpaper.dataset.selected = String(state.background.kind === "wallpaper");
  const preview = root.querySelector<HTMLImageElement>("[data-wallpaper-preview]");
  const placeholder = root.querySelector<HTMLElement>("[data-wallpaper-placeholder]");
  const change = root.querySelector<HTMLButtonElement>("[data-action='change-wallpaper']");
  if (preview) {
    preview.src = wallpaperDataUrl ?? "";
    preview.hidden = !wallpaperDataUrl;
  }
  if (placeholder) placeholder.hidden = Boolean(wallpaperDataUrl);
  if (change) change.hidden = !(wallpaperDataUrl && state.background.kind === "wallpaper");
}
