export type RelaunchAction = "open" | "ask" | "none";

export function relaunchDecision(input: { before: number[]; after: number[] }): RelaunchAction {
  if (input.after.length > 0) return "ask";
  if (input.before.length > 0) return "open";
  return "none";
}
