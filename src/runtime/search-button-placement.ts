export type SearchButtonPlacement = {
  parent: HTMLElement;
  before: HTMLElement;
};

export function searchButtonPlacement(search: HTMLElement): SearchButtonPlacement | null {
  const parent = search.parentElement;
  if (!parent) return null;
  return { parent, before: search };
}
