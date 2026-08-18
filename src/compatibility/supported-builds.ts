export type SupportedCodexBuild = {
  bundleIdentifier: string;
  appVersion: string;
  appBuild: string;
  architectures: string[];
  asarMain: string;
  expectedAsarFiles: string[];
  adapterId: string;
  selectors: string[];
  featureProbes: string[];
  signingObservations: string;
  testedMacOS: string[];
  experimental: boolean;
  notes: string;
};

export const SUPPORTED_CODEX_BUILDS: readonly SupportedCodexBuild[] = [
  {
    bundleIdentifier: "com.openai.codex",
    appVersion: "26.810.52044",
    appBuild: "6662",
    architectures: ["arm64", "x86_64"],
    asarMain: ".vite/build/early-bootstrap.js",
    expectedAsarFiles: ["package.json", ".vite/build/early-bootstrap.js"],
    adapterId: "build-26.810.52044",
    selectors: [".ms-auto.flex.items-center", ".home-banners"],
    featureProbes: ["search-aria-label", "home-banners"],
    signingObservations: "Official bundle is Developer ID signed; Incodex must ad hoc resign after asar rewrite.",
    testedMacOS: ["15"],
    experimental: true,
    notes: "Observed during the P0/P1 campaign. Live install remains experimental.",
  },
];

export function findSupportedBuild(input: {
  bundleIdentifier?: string | null;
  appVersion?: string | null;
  appBuild?: string | null;
}): SupportedCodexBuild | undefined {
  return SUPPORTED_CODEX_BUILDS.find(
    (entry) =>
      entry.bundleIdentifier === input.bundleIdentifier &&
      entry.appVersion === input.appVersion &&
      entry.appBuild === input.appBuild,
  );
}

export function assertLiveSupported(input: {
  bundleIdentifier?: string | null;
  appVersion?: string | null;
  appBuild?: string | null;
}): SupportedCodexBuild {
  const known = findSupportedBuild(input);
  if (known) return known;
  const version = input.appVersion ?? "unknown";
  const build = input.appBuild ?? "unknown";
  throw new Error(
    `unknown Codex build ${version} (${build}); refusing --live. Use install --clone for experimental mode.`,
  );
}
