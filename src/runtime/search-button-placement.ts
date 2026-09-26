export type SearchButtonPlacement = {
  parent: HTMLElement;
  before: HTMLElement;
};

const TOOLTIP_TRIGGER_STATES = new Set(["closed", "delayed-open", "instant-open"]);
const INJECTED_PRIVACY_TOGGLE_ATTRIBUTE = "data-incodex-privacy-toggle";

function isSearchTooltipTrigger(element: HTMLElement): boolean {
  const state = element.getAttribute("data-state");
  return element.tagName === "SPAN" && state !== null && TOOLTIP_TRIGGER_STATES.has(state);
}

function isButtonControl(element: Element): boolean {
  return element.tagName === "BUTTON" || element.getAttribute("role") === "button";
}

function toolbarActionCount(element: Element): number {
  if (element.hasAttribute(INJECTED_PRIVACY_TOGGLE_ATTRIBUTE)) return 0;
  if (isButtonControl(element)) return 1;

  let count = 0;
  for (const child of Array.from(element.children)) {
    count += toolbarActionCount(child);
    if (count > 1) return count;
  }
  return count;
}

function directChildContaining(parent: HTMLElement, descendant: HTMLElement): HTMLElement | null {
  let branch = descendant;
  while (branch.parentElement && branch.parentElement !== parent) {
    branch = branch.parentElement;
  }
  return branch.parentElement === parent ? branch : null;
}

function groupedToolbarPlacement(search: HTMLElement): SearchButtonPlacement | null {
  for (let group = search.parentElement; group; group = group.parentElement) {
    if (group.tagName === "BODY" || group.tagName === "HTML") break;
    const searchBranch = directChildContaining(group, search);
    if (!searchBranch) continue;

    const actionBranches = Array.from(group.children).filter(
      (child) => !child.hasAttribute(INJECTED_PRIVACY_TOGGLE_ATTRIBUTE),
    );
    if (actionBranches.length < 2 || !actionBranches.includes(searchBranch) ||
        actionBranches.some((child) => toolbarActionCount(child) !== 1)) continue;

    return { parent: group, before: actionBranches[0] as HTMLElement };
  }
  return null;
}

export function searchButtonPlacement(search: HTMLElement): SearchButtonPlacement | null {
  const parent = search.parentElement;
  if (!parent) return null;

  const groupedPlacement = groupedToolbarPlacement(search);
  if (groupedPlacement) return groupedPlacement;

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
