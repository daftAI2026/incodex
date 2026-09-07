const OFFICIAL_WINDOW_ZOOM_PROPERTY = "--codex-window-zoom";

export function parseOfficialWindowZoom(value: string): number {
  const zoom = Number.parseFloat(value);
  return Number.isFinite(zoom) && zoom > 0 ? zoom : 1;
}

export function officialWindowZoom(root: HTMLElement): number {
  return parseOfficialWindowZoom(
    window.getComputedStyle(root).getPropertyValue(OFFICIAL_WINDOW_ZOOM_PROPERTY),
  );
}

export type OfficialTooltipPresentation = {
  className: string;
  shortcutClassName: string;
};

// Keep semantic classes, so theme/CSS variable changes remain live. Never
// inspect arbitrary page tooltips: the official trigger must describe it.
export function createOfficialTooltipPresentation(): {
  read: (trigger: HTMLElement | null) => OfficialTooltipPresentation | null;
} {
  let sampledTrigger: HTMLElement | null = null;
  let sample: OfficialTooltipPresentation | null = null;
  return {
    read(trigger) {
      if (!trigger?.isConnected) {
        sampledTrigger = null;
        sample = null;
        return null;
      }
      if (trigger !== sampledTrigger) {
        sampledTrigger = trigger;
        sample = null;
      }
      const tip = findOfficialTooltipElement(trigger);
      if (tip) {
        sample = {
          className: tip.className,
          shortcutClassName: tip.querySelector("kbd")?.className ?? "",
        };
      }
      return sample;
    },
  };
}

export function findOfficialTooltipElement(trigger: HTMLElement | null): HTMLElement | null {
  if (!trigger?.isConnected) return null;
  const ids = [trigger, trigger.parentElement]
    .flatMap((element) => element?.getAttribute("aria-describedby")?.split(/\s+/) ?? [])
    .filter(Boolean);
  for (const id of ids) {
    const tip = trigger.ownerDocument.getElementById(id);
    if (
      tip?.isConnected &&
      tip.getAttribute("role") === "tooltip" &&
      !tip.hasAttribute("data-incodex-tooltip") &&
      tip.className.trim()
    ) return tip;
  }
  return null;
}
