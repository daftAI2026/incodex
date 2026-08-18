export type UiAdapter = {
  id: string;
  appVersion: string;
  appBuild: string;
  selectors: {
    headerCluster: string;
    homeBanners: string;
  };
  stripCloneAttrs: string[];
  probeUi(root: ParentNode): { search: boolean; banners: boolean };
};
