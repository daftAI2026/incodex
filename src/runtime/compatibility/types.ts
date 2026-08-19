export type UiAdapter = {
  id: string;
  selectors: {
    headerCluster: string;
    homeBanners: string;
  };
  stripCloneAttrs: string[];
  probeUi(root: ParentNode): { search: boolean; banners: boolean };
};
