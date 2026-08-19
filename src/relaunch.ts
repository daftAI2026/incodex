export type RelaunchAction = "open" | "none";

export function relaunchDecision(input: {
  before: number[];
  after: number[];
  skipped?: boolean;
}): RelaunchAction {
  if (input.skipped) return "none";
  if (input.before.length > 0 && input.after.length === 0) return "open";
  return "none";
}
