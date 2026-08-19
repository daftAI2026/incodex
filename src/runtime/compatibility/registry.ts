import { ADAPTER } from "./default-adapter";
import type { UiAdapter } from "./types";

export function activeAdapter(): UiAdapter {
  return ADAPTER;
}
