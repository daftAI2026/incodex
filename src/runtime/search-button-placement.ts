export type SearchButtonPlacement = {
  parent: HTMLElement;
  before: HTMLElement;
};

const TOOLTIP_TRIGGER_STATES = new Set(["closed", "delayed-open", "instant-open"]);

function isSearchTooltipTrigger(element: HTMLElement): boolean {
  const state = element.getAttribute("data-state");
  return element.tagName === "SPAN" && state !== null && TOOLTIP_TRIGGER_STATES.has(state);
}

export function searchButtonPlacement(search: HTMLElement): SearchButtonPlacement | null {
  const parent = search.parentElement;
  if (!parent) return null;
  if (isSearchTooltipTrigger(parent) && parent.parentElement) {
    return { parent: parent.parentElement, before: parent };
  }
  return { parent, before: search };
}

export function searchTooltipOpen(search: HTMLElement): boolean {
  const parent = search.parentElement;
  if (!parent || !isSearchTooltipTrigger(parent)) return false;
  return parent.getAttribute("data-state") !== "closed" || parent.hasAttribute("aria-describedby");
}
