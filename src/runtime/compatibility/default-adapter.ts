import { isSearchLabel } from "./search-labels";
import type { UiAdapter } from "./types";

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
      search: buttons.some((btn) => isSearchLabel(btn.getAttribute("aria-label"))),
      banners: Boolean(root.querySelector?.(ADAPTER.selectors.homeBanners)),
    };
  },
};

export const probeUi = (root: ParentNode) => ADAPTER.probeUi(root);

