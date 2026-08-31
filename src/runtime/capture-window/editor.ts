import { captureWindowCopy, type CaptureWindowCopy } from "./copy.ts";
import { capturePhysicalPadding, renderCaptureToCanvas } from "./compositor.ts";
import { anchoredPanForZoom, viewportRectToSource } from "./geometry.ts";
import {
  applyCaptureCommand,
  CAPTURE_MAX_ZOOM,
  CAPTURE_MIN_ZOOM,
  createCaptureWindowState,
  type CapturePresetId,
  type CaptureRect,
  type CaptureRedactionStyle,
  type CaptureWindowCommand,
  type CaptureWindowState,
} from "./model.ts";
import { captureWindowTemplate } from "./view.ts";

const MAX_WALLPAPER_BYTES = 32 * 1024 * 1024;
const WALLPAPER_TYPES = new Set(["image/jpeg", "image/png", "image/webp"]);

export type CaptureWindowEditorOptions = {
  automaticRegions?: CaptureRect[];
  initialState?: CaptureWindowState;
  locale?: string;
  onClose?: () => void;
  onCopy?: (png: Blob) => Promise<void>;
  onDetectRegions?: (
    source: HTMLCanvasElement,
    revision: number,
  ) => CaptureRect[] | Promise<CaptureRect[]>;
  onNotify?: (message: string) => void;
  onRetake?: (revision: number) => HTMLCanvasElement | Promise<HTMLCanvasElement>;
  onSave?: (png: Blob, suggestedName: string) => Promise<"cancelled" | "saved">;
  source: HTMLCanvasElement;
};

export type CaptureWindowEditorController = {
  destroy: () => void;
  getState: () => CaptureWindowState;
};

type PointerGesture = {
  pointerId: number;
  startClientX: number;
  startClientY: number;
  startPanX: number;
  startPanY: number;
};

export function mountCaptureWindowEditor(
  host: HTMLElement,
  options: CaptureWindowEditorOptions,
): CaptureWindowEditorController {
  let source = options.source;
  let state =
    options.initialState ??
    createCaptureWindowState({
      height: source.height,
      scaleFactor: window.devicePixelRatio || 1,
      width: source.width,
    });
  let wallpaperImage: HTMLImageElement | null = null;
  let panX = 0;
  let panY = 0;
  let destroyed = false;
  const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const locale = options.locale ?? document.documentElement.lang ?? navigator.language;
  const copy = captureWindowCopy(locale);
  let automaticRegions = options.automaticRegions ?? [];

  const root = document.createElement("div");
  root.className = "incodex-capture-root";
  root.setAttribute("data-incodex-capture", "true");
  root.setAttribute("data-state", "editing");
  host.append(root);
  const resizeObserver = new ResizeObserver(() => {
    const canvas = root.querySelector<HTMLCanvasElement>(".incodex-capture-canvas");
    const frame = root.querySelector<HTMLElement>(".incodex-capture-canvas-frame");
    if (!canvas) return;
    window.requestAnimationFrame(() => fitCanvas(root, canvas, frame, state.zoom, panX, panY));
  });
  resizeObserver.observe(root);

  function dispatch(command: CaptureWindowCommand): void {
    state = applyCaptureCommand(state, command);
    render();
  }

  function preview(command: CaptureWindowCommand): void {
    state = applyCaptureCommand(state, command);
    renderCanvas();
  }

  function close(): void {
    if (destroyed) return;
    destroyed = true;
    resizeObserver.disconnect();
    root.remove();
    if (previousFocus?.isConnected) previousFocus.focus();
    options.onClose?.();
  }

  function notify(message: string): void {
    options.onNotify?.(message);
  }

  function setPhase(phase: "composing" | "editing" | "recapturing"): void {
    root.setAttribute("data-state", phase);
    root.toggleAttribute("aria-busy", phase !== "editing");
    if (phase === "editing") {
      render();
      return;
    }
    for (const control of root.querySelectorAll<HTMLInputElement | HTMLButtonElement>(
      "button, input",
    )) {
      control.disabled = true;
    }
  }

  function render(): void {
    if (destroyed) return;
    root.innerHTML = captureWindowTemplate(state, copy, automaticRegions.length);
    root.setAttribute("data-tool", state.tool);
    root.querySelector<HTMLElement>(".incodex-capture-backdrop")?.addEventListener("click", close);
    root.querySelector<HTMLElement>("[data-action='close']")?.addEventListener("click", close);
    root.querySelector<HTMLElement>("[data-action='cancel']")?.addEventListener("click", close);

    const rendered = renderCanvas();
    const frame = root.querySelector<HTMLElement>(".incodex-capture-canvas-frame");
    wireActions(root, dispatch, preview);
    wireStage(root, rendered, frame, state, dispatch, () => ({ panX, panY }), (x, y) => {
      panX = x;
      panY = y;
    });
    wireInputs(root, dispatch, preview, loadWallpaper, setPrivacy);
    wireKeyboard(root, state, dispatch, close, exportCopy);
    root.querySelector<HTMLElement>("[data-action='retake']")?.addEventListener("click", () => {
      void retake();
    });
    root.querySelector<HTMLElement>("[data-action='copy']")?.addEventListener("click", () => {
      void exportCopy();
    });
    root.querySelector<HTMLElement>("[data-action='save']")?.addEventListener("click", () => {
      void exportSave();
    });
    window.requestAnimationFrame(() => fitCanvas(root, rendered, frame, state.zoom, panX, panY));
  }

  function renderCanvas(): HTMLCanvasElement {
    const rendered = renderCaptureToCanvas(source, state, {
      automaticRegions,
      backgroundImage: wallpaperImage,
    });
    rendered.className = "incodex-capture-canvas";
    rendered.setAttribute("data-capture-output", "true");
    const frame = root.querySelector<HTMLElement>(".incodex-capture-canvas-frame");
    const current = frame?.querySelector<HTMLCanvasElement>(".incodex-capture-canvas");
    const canvas = current ?? rendered;
    if (current) {
      current.width = rendered.width;
      current.height = rendered.height;
      current.getContext("2d")?.drawImage(rendered, 0, 0);
    } else {
      frame?.prepend(rendered);
    }
    mountRegionLayer(frame, canvas, state, automaticRegions, copy);
    const paddingValue = root.querySelector<HTMLElement>("[data-value='padding']");
    if (paddingValue) paddingValue.textContent = `${state.padding}px`;
    window.requestAnimationFrame(() => fitCanvas(root, rendered, frame, state.zoom, panX, panY));
    return canvas;
  }

  async function retake(): Promise<void> {
    setPhase("recapturing");
    const revision = state.sourceRevision + 1;
    try {
      const nextSource = await options.onRetake?.(revision);
      if (nextSource) source = nextSource;
      automaticRegions = await detectRegions(revision);
      state = applyCaptureCommand(state, {
        kind: "retake",
        source: {
          height: source.height,
          scaleFactor: state.source.scaleFactor,
          width: source.width,
        },
      });
    } catch {
      notify(copy.retakeFailed);
    } finally {
      setPhase("editing");
    }
  }

  async function setPrivacy(enabled: boolean): Promise<void> {
    dispatch({ enabled, kind: "set-privacy" });
    await retake();
  }

  async function detectRegions(revision: number): Promise<CaptureRect[]> {
    if (!options.onDetectRegions) {
      return automaticRegions;
    }
    return options.onDetectRegions(source, revision);
  }

  async function loadWallpaper(file: File): Promise<void> {
    if (!WALLPAPER_TYPES.has(file.type) || file.size > MAX_WALLPAPER_BYTES) {
      notify(copy.wallpaperTooLarge);
      return;
    }
    try {
      const dataUrl = await readFileAsDataUrl(file);
      wallpaperImage = await loadImage(dataUrl);
      dispatch({ kind: "set-background", background: { dataUrl, kind: "wallpaper" } });
    } catch {
      notify(copy.wallpaperUnreadable);
    }
  }

  async function exportCopy(): Promise<void> {
    setPhase("composing");
    const canvas = renderCaptureToCanvas(source, state, {
      automaticRegions,
      backgroundImage: wallpaperImage,
    });
    try {
      const blob = await canvasBlob(canvas);
      if (options.onCopy) await options.onCopy(blob);
      else await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
      notify(copy.copied);
      close();
    } catch {
      setPhase("editing");
      notify(copy.clipboardUnavailable);
    }
  }

  async function exportSave(): Promise<void> {
    setPhase("composing");
    const canvas = renderCaptureToCanvas(source, state, {
      automaticRegions,
      backgroundImage: wallpaperImage,
    });
    try {
      const blob = await canvasBlob(canvas);
      const suggestedName = `Incodex ${timestampForFileName(new Date())}.png`;
      if (options.onSave) {
        const result = await options.onSave(blob, suggestedName);
        if (result === "cancelled") {
          setPhase("editing");
          return;
        }
      } else {
        downloadBlob(blob, suggestedName);
      }
      notify(copy.saved);
      close();
    } catch {
      setPhase("editing");
      notify(copy.saveFailed);
    }
  }

  render();

  return {
    destroy: close,
    getState: () => state,
  };
}

function wireActions(
  root: HTMLElement,
  dispatch: (command: CaptureWindowCommand) => void,
  preview: (command: CaptureWindowCommand) => void,
): void {
  const actions: Record<string, CaptureWindowCommand> = {
    clear: { kind: "clear-regions" },
    redo: { kind: "redo" },
    "tool-move": { kind: "set-tool", tool: "move" },
    "tool-redact": { kind: "set-tool", tool: "redact" },
    undo: { kind: "undo" },
  };
  for (const [action, command] of Object.entries(actions)) {
    root.querySelector<HTMLElement>(`[data-action='${action}']`)?.addEventListener("click", () => {
      dispatch(command);
    });
  }
  for (const style of ["mosaic", "blur", "solid"] as const) {
    root.querySelector<HTMLElement>(`[data-action='style-${style}']`)?.addEventListener("click", () => {
      updateRedactionStyle(root, style, preview);
    });
  }
  root.querySelector<HTMLElement>("[data-action='zoom-in']")?.addEventListener("click", () => {
    dispatch({ kind: "set-zoom", zoom: currentZoom(root) + 0.1 });
  });
  root.querySelector<HTMLElement>("[data-action='zoom-out']")?.addEventListener("click", () => {
    dispatch({ kind: "set-zoom", zoom: currentZoom(root) - 0.1 });
  });
  for (const option of root.querySelectorAll<HTMLElement>("[data-background]")) {
    option.addEventListener("click", () => {
      const id = option.dataset.background;
      if (id === "transparent") {
        dispatch({ kind: "set-background", background: { kind: "transparent" } });
        return;
      }
      dispatch({
        kind: "set-background",
        background: { id: id as CapturePresetId, kind: "preset" },
      });
    });
  }
}

function updateRedactionStyle(
  root: HTMLElement,
  style: CaptureRedactionStyle,
  preview: (command: CaptureWindowCommand) => void,
): void {
  preview({ kind: "set-redaction-style", style });
  for (const option of root.querySelectorAll<HTMLElement>("[data-redaction-style]")) {
    option.setAttribute("aria-pressed", String(option.dataset.redactionStyle === style));
  }
  const solidColor = root.querySelector<HTMLElement>("[data-solid-color-row]");
  if (solidColor) solidColor.hidden = style !== "solid";
}

function wireInputs(
  root: HTMLElement,
  dispatch: (command: CaptureWindowCommand) => void,
  preview: (command: CaptureWindowCommand) => void,
  loadWallpaper: (file: File) => Promise<void>,
  setPrivacy: (enabled: boolean) => Promise<void>,
): void {
  root.querySelector<HTMLInputElement>("[data-input='privacy']")?.addEventListener("change", (event) => {
    void setPrivacy((event.currentTarget as HTMLInputElement).checked);
  });
  root.querySelector<HTMLInputElement>("[data-input='shadow']")?.addEventListener("change", (event) => {
    dispatch({ kind: "set-shadow", shadow: (event.currentTarget as HTMLInputElement).checked });
  });
  const padding = root.querySelector<HTMLInputElement>("[data-input='padding']");
  padding?.addEventListener("input", (event) => {
    preview({
      kind: "set-padding",
      padding: Number.parseInt((event.currentTarget as HTMLInputElement).value, 10),
    });
  });
  padding?.addEventListener("change", (event) => {
    dispatch({
      kind: "set-padding",
      padding: Number.parseInt((event.currentTarget as HTMLInputElement).value, 10),
    });
  });
  const color = root.querySelector<HTMLInputElement>("[data-input='color']");
  color?.addEventListener("input", (event) => {
    preview({
      kind: "set-background",
      background: { color: (event.currentTarget as HTMLInputElement).value, kind: "color" },
    });
  });
  color?.addEventListener("change", (event) => {
    dispatch({
      kind: "set-background",
      background: { color: (event.currentTarget as HTMLInputElement).value, kind: "color" },
    });
  });
  const solidColor = root.querySelector<HTMLInputElement>("[data-input='solid-color']");
  solidColor?.addEventListener("input", (event) => {
    preview({
      kind: "set-solid-color",
      color: (event.currentTarget as HTMLInputElement).value,
    });
  });
  solidColor?.addEventListener("change", (event) => {
    dispatch({
      kind: "set-solid-color",
      color: (event.currentTarget as HTMLInputElement).value,
    });
  });
  root.querySelector<HTMLInputElement>("[data-input='wallpaper']")?.addEventListener("change", (event) => {
    const file = (event.currentTarget as HTMLInputElement).files?.[0];
    if (file) void loadWallpaper(file);
  });
}

function wireStage(
  root: HTMLElement,
  canvas: HTMLCanvasElement,
  frame: HTMLElement | null,
  state: CaptureWindowState,
  dispatch: (command: CaptureWindowCommand) => void,
  readPan: () => { panX: number; panY: number },
  writePan: (x: number, y: number) => void,
): void {
  const stage = root.querySelector<HTMLElement>(".incodex-capture-stage");
  if (!stage || !frame) return;
  const gestureStage = stage;
  let gesture: PointerGesture | null = null;
  let draft: HTMLElement | null = null;

  stage.addEventListener("wheel", (event) => {
    event.preventDefault();
    const nextZoom = Math.min(
      CAPTURE_MAX_ZOOM,
      Math.max(CAPTURE_MIN_ZOOM, state.zoom + (event.deltaY < 0 ? 0.1 : -0.1)),
    );
    const stageRect = stage.getBoundingClientRect();
    const currentPan = readPan();
    const pan = anchoredPanForZoom(
      { x: currentPan.panX, y: currentPan.panY },
      { x: event.clientX, y: event.clientY },
      { x: stageRect.left + stageRect.width / 2, y: stageRect.top + stageRect.height / 2 },
      state.zoom,
      nextZoom,
    );
    writePan(pan.x, pan.y);
    dispatch({ kind: "set-zoom", zoom: nextZoom });
  }, { passive: false });

  stage.addEventListener("pointerdown", (event) => {
    if (event.button !== 0 && event.button !== 1) return;
    const pan = readPan();
    gesture = {
      pointerId: event.pointerId,
      startClientX: event.clientX,
      startClientY: event.clientY,
      startPanX: pan.panX,
      startPanY: pan.panY,
    };
    stage.setPointerCapture(event.pointerId);
    if (state.tool === "redact" && event.button === 0) {
      draft = document.createElement("div");
      draft.className = "incodex-capture-draft-region";
      root.append(draft);
      updateDraft(draft, event.clientX, event.clientY, event.clientX, event.clientY);
    } else {
      stage.setAttribute("data-panning", "true");
    }
  });

  stage.addEventListener("pointermove", (event) => {
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    if (draft) {
      updateDraft(
        draft,
        gesture.startClientX,
        gesture.startClientY,
        event.clientX,
        event.clientY,
      );
      return;
    }
    const x = gesture.startPanX + event.clientX - gesture.startClientX;
    const y = gesture.startPanY + event.clientY - gesture.startClientY;
    writePan(x, y);
    frame.style.transform = `translate(${x}px, ${y}px) scale(${state.zoom})`;
  });

  function finishGesture(event: PointerEvent, commit: boolean): void {
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    if (draft) {
      if (commit) {
        const canvasRect = canvas.getBoundingClientRect();
        const scale = canvasRect.width / canvas.width;
        const rect = viewportRectToSource(
          {
            height: event.clientY - gesture.startClientY,
            width: event.clientX - gesture.startClientX,
            x: gesture.startClientX,
            y: gesture.startClientY,
          },
          {
            scale,
            x:
              canvasRect.left +
              capturePhysicalPadding(state.source, state.padding) * scale,
            y:
              canvasRect.top +
              capturePhysicalPadding(state.source, state.padding) * scale,
          },
          state.source,
        );
        dispatch({ kind: "add-region", rect });
      }
      draft.remove();
      draft = null;
    }
    gestureStage.removeAttribute("data-panning");
    if (gestureStage.hasPointerCapture(event.pointerId)) {
      gestureStage.releasePointerCapture(event.pointerId);
    }
    gesture = null;
  }

  stage.addEventListener("pointerup", (event) => {
    finishGesture(event, true);
  });
  stage.addEventListener("pointercancel", (event) => {
    finishGesture(event, false);
  });
}

function wireKeyboard(
  root: HTMLElement,
  state: CaptureWindowState,
  dispatch: (command: CaptureWindowCommand) => void,
  close: () => void,
  copy: () => Promise<void>,
): void {
  root.onkeydown = (event) => {
    const modifier = event.metaKey || event.ctrlKey;
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    if (event.key === "Tab") {
      trapTabFocus(root, event);
      return;
    }
    if (modifier && event.key.toLowerCase() === "c") {
      event.preventDefault();
      void copy();
      return;
    }
    if (!modifier || event.key.toLowerCase() !== "z") return;
    event.preventDefault();
    dispatch({ kind: event.shiftKey ? "redo" : "undo" });
  };
  root.setAttribute("tabindex", "-1");
  root.querySelector<HTMLElement>("button:not(:disabled), input:not(:disabled)")?.focus();
  root.setAttribute("data-tool", state.tool);
}

function trapTabFocus(root: HTMLElement, event: KeyboardEvent): void {
  const controls = [...root.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled)")];
  if (controls.length === 0) return;
  const current = controls.indexOf(document.activeElement as HTMLElement);
  const next = event.shiftKey
    ? controls[(current <= 0 ? controls.length : current) - 1]
    : controls[(current + 1) % controls.length];
  event.preventDefault();
  next.focus();
}

function mountRegionLayer(
  frame: HTMLElement | null,
  canvas: HTMLCanvasElement,
  state: CaptureWindowState,
  automaticRegions: CaptureRect[],
  copy: CaptureWindowCopy,
): void {
  const layer = frame?.querySelector<HTMLElement>(".incodex-capture-region-layer");
  if (!layer) return;
  layer.replaceChildren();
  const regions = state.privacyEnabled ? automaticRegions : [];
  for (const region of regions) {
    layer.append(regionElement(region, canvas, state, true, copy.automaticBadge));
  }
  for (const region of state.regions) {
    layer.append(regionElement(region, canvas, state, false, copy.automaticBadge));
  }
}

function regionElement(
  region: CaptureRect,
  canvas: HTMLCanvasElement,
  state: CaptureWindowState,
  automatic: boolean,
  automaticLabel: string,
): HTMLElement {
  const padding = capturePhysicalPadding(state.source, state.padding);
  const element = document.createElement("div");
  element.className = "incodex-capture-region";
  element.dataset.automatic = String(automatic);
  if (automatic) element.dataset.label = automaticLabel;
  element.style.left = `${((region.x + padding) / canvas.width) * 100}%`;
  element.style.top = `${((region.y + padding) / canvas.height) * 100}%`;
  element.style.width = `${(region.width / canvas.width) * 100}%`;
  element.style.height = `${(region.height / canvas.height) * 100}%`;
  return element;
}

function fitCanvas(
  root: HTMLElement,
  canvas: HTMLCanvasElement,
  frame: HTMLElement | null,
  zoom: number,
  panX: number,
  panY: number,
): void {
  const stage = root.querySelector<HTMLElement>(".incodex-capture-stage");
  if (!stage || !frame) return;
  const availableWidth = Math.max(1, stage.clientWidth - 40);
  const availableHeight = Math.max(1, stage.clientHeight - 40);
  const fit = Math.min(availableWidth / canvas.width, availableHeight / canvas.height, 1);
  frame.style.width = `${Math.round(canvas.width * fit)}px`;
  frame.style.height = `${Math.round(canvas.height * fit)}px`;
  frame.style.transform = `translate(${panX}px, ${panY}px) scale(${zoom})`;
  canvas.style.width = "100%";
  canvas.style.height = "100%";
}

function updateDraft(
  draft: HTMLElement,
  startX: number,
  startY: number,
  endX: number,
  endY: number,
): void {
  draft.style.left = `${Math.min(startX, endX)}px`;
  draft.style.top = `${Math.min(startY, endY)}px`;
  draft.style.width = `${Math.abs(endX - startX)}px`;
  draft.style.height = `${Math.abs(endY - startY)}px`;
}

function currentZoom(root: HTMLElement): number {
  const value = root.querySelector<HTMLElement>(".incodex-capture-zoom-value")?.textContent;
  return Number.parseInt(value ?? "100", 10) / 100;
}

function readFileAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.addEventListener("load", () => resolve(String(reader.result)));
    reader.addEventListener("error", () => reject(reader.error));
    reader.readAsDataURL(file);
  });
}

function loadImage(source: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.addEventListener("load", () => resolve(image));
    image.addEventListener("error", () => reject(new Error("Unable to load image")));
    image.src = source;
  });
}

function canvasBlob(canvas: HTMLCanvasElement): Promise<Blob> {
  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (blob) resolve(blob);
      else reject(new Error("Unable to encode PNG"));
    }, "image/png");
  });
}

function downloadBlob(blob: Blob, fileName: string): void {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.download = fileName;
  anchor.href = url;
  anchor.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}

function timestampForFileName(date: Date): string {
  const pad = (value: number): string => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} at ${pad(date.getHours())}.${pad(date.getMinutes())}.${pad(date.getSeconds())}`;
}
