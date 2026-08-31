export type CaptureColorTarget = "background" | "solid";

export type CaptureHsv = {
  hue: number;
  saturation: number;
  value: number;
};

export type CaptureColorPopoverOptions = {
  label: (target: CaptureColorTarget) => string;
  onChange: (target: CaptureColorTarget, color: string) => void;
  onOpen: (target: CaptureColorTarget) => void;
  readColor: (target: CaptureColorTarget) => string;
};

export function sanitizeCaptureHex(value: string): string {
  return value.replace(/([^0-9A-F]+)/gi, "").substring(0, 6);
}

export function isCaptureHexColor(value: string): boolean {
  const length = value.replace(/^#/, "").length;
  return (length === 3 || length === 6) && /^#?[0-9A-F]+$/i.test(value);
}

export function captureHsvFromHex(color: string): CaptureHsv {
  const hex = expandedHex(color);
  const red = Number.parseInt(hex.slice(0, 2), 16) / 255;
  const green = Number.parseInt(hex.slice(2, 4), 16) / 255;
  const blue = Number.parseInt(hex.slice(4, 6), 16) / 255;
  const maximum = Math.max(red, green, blue);
  const difference = maximum - Math.min(red, green, blue);
  let hue = 0;
  if (difference > 0) {
    if (maximum === red) hue = ((green - blue) / difference) % 6;
    else if (maximum === green) hue = (blue - red) / difference + 2;
    else hue = (red - green) / difference + 4;
    hue = (hue * 60 + 360) % 360;
  }
  return {
    hue,
    saturation: maximum === 0 ? 0 : (difference / maximum) * 100,
    value: maximum * 100,
  };
}

export function captureHexFromHsv(color: CaptureHsv): string {
  const hue = ((color.hue % 360) + 360) % 360;
  const saturation = clamp(color.saturation, 0, 100) / 100;
  const value = clamp(color.value, 0, 100) / 100;
  const chroma = value * saturation;
  const segment = hue / 60;
  const second = chroma * (1 - Math.abs((segment % 2) - 1));
  const [red, green, blue] = rgbSegment(Math.floor(segment), chroma, second);
  const match = value - chroma;
  return `#${hexByte(red + match)}${hexByte(green + match)}${hexByte(blue + match)}`;
}

export function captureColorPopoverTemplate(
  target: CaptureColorTarget,
  color: string,
  label: string,
): string {
  const hsv = captureHsvFromHex(color);
  const safeLabel = escapeAttribute(label);
  return `
    <div class="incodex-capture-color-popover" data-color-popover="${target}" data-capture-hide data-incodex-capture-hide role="dialog" data-side="bottom" data-align="end">
      <div class="incodex-capture-color-picker react-colorful">
        <div class="react-colorful__saturation" data-color-saturation="${target}" style="--capture-picker-hue:hsl(${hsv.hue} 100% 50%)">
          <div class="react-colorful__interactive" role="slider" tabindex="0" aria-label="Color">
            <span class="react-colorful__pointer react-colorful__saturation-pointer" style="left:${hsv.saturation}%;top:${100 - hsv.value}%"><span class="react-colorful__pointer-fill" style="background-color:${color}"></span></span>
          </div>
        </div>
        <div class="react-colorful__hue react-colorful__last-control" data-color-hue="${target}">
          <div class="react-colorful__interactive" role="slider" tabindex="0" aria-label="Hue" aria-valuemin="0" aria-valuemax="360" aria-valuenow="${Math.round(hsv.hue)}">
            <span class="react-colorful__pointer react-colorful__hue-pointer" style="left:${hsv.hue / 3.6}%;top:50%"><span class="react-colorful__pointer-fill" style="background-color:hsl(${hsv.hue} 100% 50%)"></span></span>
          </div>
        </div>
      </div>
      <input class="incodex-capture-color-hex" data-color-hex="${target}" aria-label="${safeLabel}" value="${color}" spellcheck="false">
    </div>
  `;
}

export function wireCaptureColorPopovers(
  root: HTMLElement,
  options: CaptureColorPopoverOptions,
): () => void {
  let activeTarget: CaptureColorTarget | null = null;
  let activeTrigger: HTMLButtonElement | null = null;
  let activePopover: HTMLElement | null = null;

  function close(returnFocus: boolean): void {
    activePopover?.remove();
    activeTrigger?.setAttribute("aria-expanded", "false");
    activeTrigger?.setAttribute("data-state", "closed");
    if (returnFocus) activeTrigger?.focus();
    activeTarget = null;
    activeTrigger = null;
    activePopover = null;
  }

  function open(target: CaptureColorTarget, trigger: HTMLButtonElement): void {
    close(false);
    options.onOpen(target);
    const template = document.createElement("template");
    template.innerHTML = captureColorPopoverTemplate(
      target,
      options.readColor(target),
      options.label(target),
    ).trim();
    const popover = template.content.firstElementChild;
    if (!(popover instanceof HTMLElement)) return;
    root.append(popover);
    activeTarget = target;
    activeTrigger = trigger;
    activePopover = popover;
    trigger.setAttribute("aria-expanded", "true");
    trigger.setAttribute("data-state", "open");
    trigger.setAttribute("aria-controls", `incodex-capture-color-${target}`);
    popover.id = `incodex-capture-color-${target}`;
    positionPopover(trigger, popover);
    wirePicker(popover, target, options.onChange);
  }

  function onRootClick(event: MouseEvent): void {
    const trigger = (event.target as Element | null)?.closest<HTMLButtonElement>(
      "[data-color-trigger]",
    );
    if (!trigger || !root.contains(trigger)) return;
    const target = trigger.dataset.colorTrigger as CaptureColorTarget;
    if (activeTarget === target) {
      close(false);
      return;
    }
    open(target, trigger);
  }

  function onDocumentPointerDown(event: PointerEvent): void {
    if (!activePopover || !activeTrigger) return;
    const target = event.target as Node | null;
    if (target && (activePopover.contains(target) || activeTrigger.contains(target))) return;
    close(false);
  }

  function onRootKeyDown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || !activePopover) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    close(true);
  }

  root.addEventListener("click", onRootClick);
  root.addEventListener("keydown", onRootKeyDown, true);
  document.addEventListener("pointerdown", onDocumentPointerDown, true);
  return () => {
    close(false);
    root.removeEventListener("click", onRootClick);
    root.removeEventListener("keydown", onRootKeyDown, true);
    document.removeEventListener("pointerdown", onDocumentPointerDown, true);
  };
}

export function syncCaptureColorPopover(
  root: HTMLElement,
  target: CaptureColorTarget,
  color: string,
): void {
  const trigger = root.querySelector<HTMLElement>(`[data-color-trigger="${target}"]`);
  if (target === "background") trigger?.style.setProperty("--capture-swatch", color);
  else trigger?.style.setProperty("--capture-solid-color", color);
  const popover = root.querySelector<HTMLElement>(`[data-color-popover="${target}"]`);
  if (popover) syncPicker(popover, color, document.activeElement !== hexInput(popover, target));
}

function wirePicker(
  popover: HTMLElement,
  target: CaptureColorTarget,
  onChange: (target: CaptureColorTarget, color: string) => void,
): void {
  const saturation = popover.querySelector<HTMLElement>(`[data-color-saturation="${target}"]`);
  const hue = popover.querySelector<HTMLElement>(`[data-color-hue="${target}"]`);
  const input = hexInput(popover, target);
  if (!saturation || !hue || !input) return;

  wirePickerSurface(saturation, (x, y) => {
    const current = captureHsvFromHex(currentColor(popover));
    changePicker(popover, target, { ...current, saturation: x * 100, value: (1 - y) * 100 }, onChange);
  });
  wirePickerSurface(hue, (x) => {
    const current = captureHsvFromHex(currentColor(popover));
    changePicker(popover, target, { ...current, hue: x * 360 }, onChange);
  });
  input.addEventListener("input", () => {
    const cleaned = sanitizeCaptureHex(input.value);
    input.value = `#${cleaned}`;
    if (!isCaptureHexColor(cleaned)) return;
    const color = `#${cleaned}`;
    popover.dataset.currentColor = color;
    syncPicker(popover, color, false);
    onChange(target, color);
  });
  input.addEventListener("blur", () => {
    if (!isCaptureHexColor(input.value)) input.value = currentColor(popover);
  });
  syncPicker(popover, input.value, true);
}

function wirePickerSurface(
  surface: HTMLElement,
  onMove: (x: number, y: number) => void,
): void {
  const interactive = surface.querySelector<HTMLElement>(".react-colorful__interactive");
  if (!interactive) return;
  function move(event: PointerEvent): void {
    const rect = interactive?.getBoundingClientRect();
    if (!rect) return;
    onMove(
      clamp((event.clientX - rect.left) / rect.width, 0, 1),
      clamp((event.clientY - rect.top) / rect.height, 0, 1),
    );
  }
  interactive.addEventListener("pointerdown", (event) => {
    interactive.setPointerCapture(event.pointerId);
    interactive.focus();
    move(event);
  });
  interactive.addEventListener("pointermove", (event) => {
    if (interactive.hasPointerCapture(event.pointerId)) move(event);
  });
  interactive.addEventListener("pointerup", (event) => {
    if (interactive.hasPointerCapture(event.pointerId)) interactive.releasePointerCapture(event.pointerId);
  });
  interactive.addEventListener("keydown", (event) => {
    if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) return;
    event.preventDefault();
    const rect = interactive.getBoundingClientRect();
    const pointer = interactive.querySelector<HTMLElement>(".react-colorful__pointer");
    const left = Number.parseFloat(pointer?.style.left ?? "0") / 100;
    const top = Number.parseFloat(pointer?.style.top ?? "50") / 100;
    const { horizontal, vertical } = arrowMovement(event.key);
    if (rect.width > 0 && rect.height > 0) onMove(clamp(left + horizontal, 0, 1), clamp(top + vertical, 0, 1));
  });
}

function changePicker(
  popover: HTMLElement,
  target: CaptureColorTarget,
  hsv: CaptureHsv,
  onChange: (target: CaptureColorTarget, color: string) => void,
): void {
  const color = captureHexFromHsv(hsv);
  syncPicker(popover, color, true);
  onChange(target, color);
}

function syncPicker(popover: HTMLElement, color: string, updateInput: boolean): void {
  const target = popover.dataset.colorPopover as CaptureColorTarget;
  const hsv = captureHsvFromHex(color);
  popover.dataset.currentColor = color;
  const saturation = popover.querySelector<HTMLElement>(`[data-color-saturation="${target}"]`);
  const saturationPointer = saturation?.querySelector<HTMLElement>(".react-colorful__pointer");
  const saturationFill = saturationPointer?.querySelector<HTMLElement>(".react-colorful__pointer-fill");
  saturation?.style.setProperty("--capture-picker-hue", `hsl(${hsv.hue} 100% 50%)`);
  if (saturationPointer) {
    saturationPointer.style.left = `${hsv.saturation}%`;
    saturationPointer.style.top = `${100 - hsv.value}%`;
  }
  if (saturationFill) saturationFill.style.backgroundColor = color;
  const hue = popover.querySelector<HTMLElement>(`[data-color-hue="${target}"]`);
  const huePointer = hue?.querySelector<HTMLElement>(".react-colorful__pointer");
  const hueFill = huePointer?.querySelector<HTMLElement>(".react-colorful__pointer-fill");
  hue?.querySelector<HTMLElement>(".react-colorful__interactive")?.setAttribute("aria-valuenow", String(Math.round(hsv.hue)));
  if (huePointer) huePointer.style.left = `${hsv.hue / 3.6}%`;
  if (hueFill) hueFill.style.backgroundColor = `hsl(${hsv.hue} 100% 50%)`;
  const input = hexInput(popover, target);
  if (input && updateInput) input.value = color;
}

function positionPopover(trigger: HTMLElement, popover: HTMLElement): void {
  const triggerRect = trigger.getBoundingClientRect();
  const gap = 4;
  const edge = 8;
  let top = triggerRect.bottom + gap;
  let side = "bottom";
  if (top + popover.offsetHeight > window.innerHeight - edge) {
    top = triggerRect.top - gap - popover.offsetHeight;
    side = "top";
  }
  const left = clamp(triggerRect.right - popover.offsetWidth, edge, window.innerWidth - popover.offsetWidth - edge);
  popover.style.left = `${left}px`;
  popover.style.top = `${Math.max(edge, top)}px`;
  popover.dataset.side = side;
}

function currentColor(popover: HTMLElement): string {
  return popover.dataset.currentColor ?? "#000000";
}

function arrowMovement(key: string): { horizontal: number; vertical: number } {
  switch (key) {
    case "ArrowLeft":
      return { horizontal: -0.05, vertical: 0 };
    case "ArrowRight":
      return { horizontal: 0.05, vertical: 0 };
    case "ArrowUp":
      return { horizontal: 0, vertical: -0.05 };
    case "ArrowDown":
      return { horizontal: 0, vertical: 0.05 };
    default:
      return { horizontal: 0, vertical: 0 };
  }
}

function hexInput(popover: ParentNode, target: CaptureColorTarget): HTMLInputElement | null {
  return popover.querySelector<HTMLInputElement>(`[data-color-hex="${target}"]`);
}

function expandedHex(color: string): string {
  const sanitized = sanitizeCaptureHex(color);
  if (sanitized.length === 3) return sanitized.split("").map((part) => part + part).join("");
  return sanitized.padEnd(6, "0").slice(0, 6);
}

function rgbSegment(segment: number, chroma: number, second: number): [number, number, number] {
  switch (segment % 6) {
    case 0:
      return [chroma, second, 0];
    case 1:
      return [second, chroma, 0];
    case 2:
      return [0, chroma, second];
    case 3:
      return [0, second, chroma];
    case 4:
      return [second, 0, chroma];
    default:
      return [chroma, 0, second];
  }
}

function hexByte(value: number): string {
  return Math.round(value * 255).toString(16).padStart(2, "0");
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}

function escapeAttribute(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll('"', "&quot;").replaceAll("<", "&lt;");
}
