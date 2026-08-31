import { captureWindowCopy, type CaptureWindowCopy } from "./copy.ts";
import { capturePhysicalPadding, renderCaptureToCanvas } from "./compositor.ts";
import { anchoredPanForZoom, viewportRectToSource } from "./geometry.ts";
import {
  applyCaptureCommand,
  type CaptureCandidate,
  captureHistoryShortcut,
  capturePointerIntent,
  createCaptureWindowState,
  type CapturePresetId,
  type CaptureRect,
  type CaptureRedactionStyle,
  scaleCaptureZoom,
  type CaptureWindowCommand,
  type CaptureWindowState,
  wheelCaptureZoom,
} from "./model.ts";
import { captureWindowTemplate } from "./view.ts";

const MAX_WALLPAPER_BYTES = 32 * 1024 * 1024;
const WALLPAPER_TYPES = new Set(["image/jpeg", "image/png", "image/webp"]);

export type CaptureWindowEditorOptions = {
  automaticRegions?: CaptureCandidate[];
  initialState?: CaptureWindowState;
  locale?: string;
  onClose?: () => void;
  onCopy?: (png: Blob) => Promise<void>;
  onDetectRegions?: (
    source: HTMLCanvasElement,
    revision: number,
  ) => CaptureCandidate[] | Promise<CaptureCandidate[]>;
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
  let manualRegionSequence = 0;
  let destroyed = false;
  const previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const locale = options.locale ?? document.documentElement.lang ?? navigator.language;
  const copy = captureWindowCopy(locale);
  let automaticCandidates = options.automaticRegions ?? [];

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

  function dispatchRegion(command: CaptureWindowCommand): void {
    state = applyCaptureCommand(state, command);
    renderCanvas();
    updateHistoryControls(root, state);
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
    root.innerHTML = captureWindowTemplate(state, copy);
    root.setAttribute("data-tool", state.tool);
    root.querySelector<HTMLElement>(".incodex-capture-backdrop")?.addEventListener("click", close);
    root.querySelector<HTMLElement>("[data-action='close']")?.addEventListener("click", close);

    const rendered = renderCanvas();
    const frame = root.querySelector<HTMLElement>(".incodex-capture-canvas-frame");
    wireActions(root, dispatch, dispatchRegion, preview, () => {
      panX = 0;
      panY = 0;
      dispatch({ kind: "set-zoom", zoom: 1 });
    });
    wireStage(
      root,
      rendered,
      frame,
      state,
      dispatchRegion,
      () => `manual-${++manualRegionSequence}`,
      () => ({ panX, panY }),
      (x, y) => {
        panX = x;
        panY = y;
      },
    );
    wireInputs(root, dispatch, preview, loadWallpaper, setPrivacy);
    wireKeyboard(root, state, dispatchRegion, close, exportCopy);
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
    mountRegionLayer(frame, canvas, state, automaticCandidates, copy, dispatchRegion);
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
      automaticCandidates = await detectRegions(revision);
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

  async function detectRegions(revision: number): Promise<CaptureCandidate[]> {
    if (!options.onDetectRegions) {
      return automaticCandidates;
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
  dispatchRegion: (command: CaptureWindowCommand) => void,
  preview: (command: CaptureWindowCommand) => void,
  resetView: () => void,
): void {
  const actions: Record<string, CaptureWindowCommand> = {
    "source-auto": { kind: "set-redaction-source", source: "auto" },
    "source-draw": { kind: "set-redaction-source", source: "draw" },
    "tool-move": { kind: "set-tool", tool: "move" },
    "tool-redact": { kind: "set-tool", tool: "redact" },
  };
  for (const [action, command] of Object.entries(actions)) {
    root.querySelector<HTMLElement>(`[data-action='${action}']`)?.addEventListener("click", () => {
      dispatch(command);
    });
  }
  const regionActions: Record<string, CaptureWindowCommand> = {
    redo: { kind: "redo" },
    undo: { kind: "undo" },
  };
  for (const [action, command] of Object.entries(regionActions)) {
    root.querySelector<HTMLElement>(`[data-action='${action}']`)?.addEventListener("click", () => {
      dispatchRegion(command);
    });
  }
  for (const style of ["mosaic", "blur", "solid"] as const) {
    root.querySelector<HTMLElement>(`[data-action='style-${style}']`)?.addEventListener("click", () => {
      updateRedactionStyle(root, style, preview);
    });
  }
  root.querySelector<HTMLElement>("[data-action='zoom-in']")?.addEventListener("click", () => {
    dispatch({ kind: "set-zoom", zoom: scaleCaptureZoom(currentZoom(root), 1.25) });
  });
  root.querySelector<HTMLElement>("[data-action='zoom-out']")?.addEventListener("click", () => {
    dispatch({ kind: "set-zoom", zoom: scaleCaptureZoom(currentZoom(root), 1 / 1.25) });
  });
  root.querySelector<HTMLElement>("[data-action='zoom-reset']")?.addEventListener("click", () => {
    resetView();
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
  createManualRegionId: () => string,
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
    const nextZoom = wheelCaptureZoom(state.zoom, event.deltaY);
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
    const target = event.target instanceof Element ? event.target : null;
    const intent = capturePointerIntent(
      state.tool,
      event.button,
      Boolean(target?.closest("[data-region]")),
    );
    if (intent === "ignore" || intent === "region") return;
    const pan = readPan();
    gesture = {
      pointerId: event.pointerId,
      startClientX: event.clientX,
      startClientY: event.clientY,
      startPanX: pan.panX,
      startPanY: pan.panY,
    };
    stage.setPointerCapture(event.pointerId);
    if (intent === "draw") {
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
        dispatch({ id: createManualRegionId(), kind: "add-region", rect });
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
    const historyCommand = captureHistoryShortcut(event.key, {
      control: event.ctrlKey,
      modifier,
      shift: event.shiftKey,
    });
    if (!historyCommand) return;
    event.preventDefault();
    dispatch({ kind: historyCommand });
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
  automaticCandidates: CaptureCandidate[],
  copy: CaptureWindowCopy,
  dispatch: (command: CaptureWindowCommand) => void,
): void {
  const layer = frame?.querySelector<HTMLElement>(".incodex-capture-region-layer");
  if (!layer) return;
  layer.replaceChildren();
  if (state.tool !== "redact") return;
  const selectedAutomaticIds = new Set(
    state.regions
      .filter((region) => region.source === "automatic")
      .map((region) => region.id),
  );
  if (state.privacyEnabled) {
    for (const candidate of automaticCandidates) {
      if (selectedAutomaticIds.has(candidate.id)) continue;
      layer.append(candidateElement(candidate, canvas, state, copy, dispatch));
    }
  }
  for (const region of state.regions) {
    if (!state.privacyEnabled && region.source === "automatic") continue;
    layer.append(regionElement(region.id, region.rect, canvas, state, region.source, copy, dispatch));
  }
}

function regionElement(
  id: string,
  region: CaptureRect,
  canvas: HTMLCanvasElement,
  state: CaptureWindowState,
  source: "automatic" | "manual",
  copy: CaptureWindowCopy,
  dispatch: (command: CaptureWindowCommand) => void,
): HTMLElement {
  const padding = capturePhysicalPadding(state.source, state.padding);
  const element = document.createElement("div");
  element.className = "incodex-capture-region incodex-capture-region-confirmed";
  element.dataset.region = "";
  element.dataset.source = source;
  element.dataset.interactive = String(state.redactionSource === "auto");
  element.setAttribute("role", "button");
  element.title = copy.regionRemove;
  positionRegionElement(element, region, canvas, padding);
  const remove = document.createElement("span");
  remove.className = "incodex-capture-region-remove";
  remove.textContent = "×";
  element.append(remove);
  if (state.redactionSource === "auto") {
    element.addEventListener("pointerdown", (event) => event.stopPropagation());
    element.addEventListener("click", (event) => {
      event.stopPropagation();
      dispatch({ id, kind: "remove-region" });
    });
  }
  return element;
}

function candidateElement(
  candidate: CaptureCandidate,
  canvas: HTMLCanvasElement,
  state: CaptureWindowState,
  copy: CaptureWindowCopy,
  dispatch: (command: CaptureWindowCommand) => void,
): HTMLElement {
  const padding = capturePhysicalPadding(state.source, state.padding);
  const element = document.createElement("div");
  element.className = "incodex-capture-region incodex-capture-region-candidate";
  element.dataset.region = "";
  element.dataset.interactive = String(state.redactionSource === "auto");
  element.setAttribute("role", "button");
  element.title = copy.regionSuggestion;
  positionRegionElement(element, candidate, canvas, padding);
  if (state.redactionSource === "auto") {
    element.addEventListener("pointerdown", (event) => event.stopPropagation());
    element.addEventListener("click", (event) => {
      event.stopPropagation();
      const { id, ...rect } = candidate;
      dispatch({ id, kind: "select-automatic-region", rect });
    });
  }
  return element;
}

function updateHistoryControls(root: HTMLElement, state: CaptureWindowState): void {
  const undo = root.querySelector<HTMLButtonElement>("[data-action='undo']");
  const redo = root.querySelector<HTMLButtonElement>("[data-action='redo']");
  if (undo) undo.disabled = state.history.past.length === 0;
  if (redo) redo.disabled = state.history.future.length === 0;
}

function positionRegionElement(
  element: HTMLElement,
  region: CaptureRect,
  canvas: HTMLCanvasElement,
  padding: number,
): void {
  element.style.left = `${((region.x + padding) / canvas.width) * 100}%`;
  element.style.top = `${((region.y + padding) / canvas.height) * 100}%`;
  element.style.width = `${(region.width / canvas.width) * 100}%`;
  element.style.height = `${(region.height / canvas.height) * 100}%`;
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
  const value = root.querySelector<HTMLElement>(".incodex-capture-zoom-reset")?.textContent;
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
