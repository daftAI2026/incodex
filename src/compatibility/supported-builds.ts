export type SupportedCodexBuild = {
  bundleIdentifier: string;
  appVersion: string;
  appBuild: string;
  architectures: string[];
  asarMain: string;
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
    experimental: true,
    notes: "Observed during the P0/P1 campaign. Live install remains experimental.",
  },
];

export function findSupportedBuild(input: {
  bundleIdentifier?: string;
  appVersion?: string;
  appBuild?: string;
}): SupportedCodexBuild | undefined {
  return SUPPORTED_CODEX_BUILDS.find(
    (entry) =>
      entry.bundleIdentifier === input.bundleIdentifier &&
      entry.appVersion === input.appVersion &&
      entry.appBuild === input.appBuild,
  );
}
