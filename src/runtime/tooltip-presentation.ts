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
