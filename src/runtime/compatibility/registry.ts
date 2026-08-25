import { ADAPTER } from "./default-adapter.ts";
import type { UiAdapter } from "./types.ts";

export function activeAdapter(): UiAdapter {
  return ADAPTER;
}
