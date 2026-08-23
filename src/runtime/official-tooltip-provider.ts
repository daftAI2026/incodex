export type OfficialTooltipProvider = {
  getOpenDelay: (key: string, fallbackMs: number) => number;
  activateTooltip: (
    id: string,
    key: string,
    variant: string,
    close: () => void,
  ) => void;
  deactivateTooltip: (id: string) => void;
};

export function findOfficialTooltipProvider(_trigger: HTMLElement): OfficialTooltipProvider | null {
  return null;
}
