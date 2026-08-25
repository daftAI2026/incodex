import { isSearchLabel } from "./search-labels.ts";
import type { UiAdapter } from "./types.ts";

export const SELECTORS = {
  headerCluster: ".ms-auto.flex.items-center",
  homeBanners: ".home-banners",
};

export const STRIP_CLONE_ATTRS = [
  "id",
  "name",
  "aria-haspopup",
  "aria-expanded",
  "aria-controls",
  "aria-describedby",
  "aria-labelledby",
  "data-state",
  "data-testid",
  "data-test-id",
  "disabled",
  "title",
  "tabindex",
];

export const ADAPTER: UiAdapter = {
  id: "default",
  selectors: SELECTORS,
  stripCloneAttrs: STRIP_CLONE_ATTRS,
  probeUi(root) {
    const buttons = [...root.querySelectorAll("button")];
    return {
      search: buttons.some((button) => isSearchLabel(button.getAttribute("aria-label"))),
      banners: Boolean(root.querySelector?.(ADAPTER.selectors.homeBanners)),
    };
  },
};

export function probeUi(root: ParentNode): { search: boolean; banners: boolean } {
  return ADAPTER.probeUi(root);
}
