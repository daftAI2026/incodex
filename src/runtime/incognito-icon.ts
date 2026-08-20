export type IncognitoButtonIcon = "hat-glasses" | "circle-x";

export function iconFor(input: { incognito: boolean; hovered: boolean }): IncognitoButtonIcon {
  return input.incognito && input.hovered ? "circle-x" : "hat-glasses";
}
