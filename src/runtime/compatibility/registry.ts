import { ADAPTER as build26810 } from "./build-26.810.52044";
import type { UiAdapter } from "./types";

const ADAPTERS: readonly UiAdapter[] = [build26810];

export function adapterForBuild(appVersion?: string, appBuild?: string): UiAdapter | undefined {
  return ADAPTERS.find((adapter) => adapter.appVersion === appVersion && adapter.appBuild === appBuild);
}

export function activeAdapter(): UiAdapter {
  return build26810;
}
