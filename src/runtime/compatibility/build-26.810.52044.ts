import { isSearchLabel } from "./search-labels";
import type { UiAdapter } from "./types";

export const ADAPTER: UiAdapter = {
  id: "build-26.810.52044",
  appVersion: "26.810.52044",
  appBuild: "6662",
  selectors: {
    headerCluster: ".ms-auto.flex.items-center",
    homeBanners: ".home-banners",
  },
  stripCloneAttrs: [
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
  ],
  probeUi(root) {
    const buttons = [...root.querySelectorAll("button")];
    return {
      search: buttons.some((btn) => isSearchLabel(btn.getAttribute("aria-label"))),
      banners: Boolean(root.querySelector?.(ADAPTER.selectors.homeBanners)),
    };
  },
};

export const SELECTORS = ADAPTER.selectors;
export const STRIP_CLONE_ATTRS = ADAPTER.stripCloneAttrs;
export const probeUi = (root: ParentNode) => ADAPTER.probeUi(root);
