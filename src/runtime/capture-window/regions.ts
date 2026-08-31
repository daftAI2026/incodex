import type { CaptureWindowCopy } from "./copy.ts";
import { capturePhysicalPadding } from "./compositor.ts";
import type {
  CaptureCandidate,
  CaptureRect,
  CaptureWindowCommand,
  CaptureWindowState,
} from "./model.ts";

export function mountCaptureRegionLayer(
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
  for (const candidate of automaticCandidates) {
    if (selectedAutomaticIds.has(candidate.id)) continue;
    layer.append(candidateElement(candidate, canvas, state, copy, dispatch));
  }
  for (const region of state.regions) {
    layer.append(
      regionElement(region.id, region.rect, canvas, state, region.source, copy, dispatch),
    );
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
