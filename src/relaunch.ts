export type RelaunchAction = "open" | "none";

export function relaunchDecision(input: { before: number[] }): RelaunchAction {
  return input.before.length > 0 ? "open" : "none";
}
