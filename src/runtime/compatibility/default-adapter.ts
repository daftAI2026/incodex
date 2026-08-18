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

export const SELECTORS = {
  headerCluster: ".ms-auto.flex.items-center",
  homeBanners: ".home-banners",
};

export function probeUi(root: ParentNode): { search: boolean; banners: boolean } {
  return {
    search: Boolean(root.querySelector?.("button")),
    banners: Boolean((root as Document).querySelector?.(SELECTORS.homeBanners)),
  };
}
