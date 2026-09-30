/**
 * [INPUT]: 依赖官方触发器 aria-describedby 关联与当前窗口缩放变量
 * [OUTPUT]: 提供 tooltip class 取样缓存、触发器更换失效和原生降级文案
 * [POS]: Runtime 视觉适配边界；保存语义 class 而非冻结计算后的颜色
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
const OFFICIAL_WINDOW_ZOOM_PROPERTY = "--codex-window-zoom";

export function parseOfficialWindowZoom(value: string): number {
  const zoom = Number.parseFloat(value);
  return Number.isFinite(zoom) && zoom > 0 ? zoom : 1;
}

export function officialWindowZoom(root: HTMLElement): number {
  return parseOfficialWindowZoom(
    window.getComputedStyle(root).getPropertyValue(OFFICIAL_WINDOW_ZOOM_PROPERTY),
  );
}

export type OfficialTooltipPresentation = {
  className: string;
  shortcutClassName: string;
  side?: "top" | "bottom";
  gap?: number;
};

// Keep semantic classes, so theme/CSS variable changes remain live. Never
// inspect arbitrary page tooltips: the official trigger must describe it.
export function createOfficialTooltipPresentation(): {
  read: (trigger: HTMLElement | null) => OfficialTooltipPresentation | null;
} {
  let sampledTrigger: HTMLElement | null = null;
  let sample: OfficialTooltipPresentation | null = null;
  return {
    read(trigger) {
      if (!trigger?.isConnected) {
        sampledTrigger = null;
        sample = null;
        return null;
      }
      if (trigger !== sampledTrigger) {
        sampledTrigger = trigger;
        sample = null;
      }
      const tip = findOfficialTooltipElement(trigger);
      if (tip) {
        const side = tip.getAttribute("data-side");
        const position: Pick<OfficialTooltipPresentation, "side" | "gap"> = {};
        if ((side === "top" || side === "bottom") &&
            typeof tip.getBoundingClientRect === "function" &&
            typeof trigger.getBoundingClientRect === "function") {
          const tipRect = tip.getBoundingClientRect();
          const triggerRect = trigger.getBoundingClientRect();
          const gap = side === "bottom" ? tipRect.top - triggerRect.bottom : triggerRect.top - tipRect.bottom;
          if (Number.isFinite(gap)) {
            position.side = side;
            position.gap = gap;
          }
        }
        sample = {
          className: tip.className,
          shortcutClassName: tip.querySelector("kbd")?.className ?? "",
          ...position,
        };
      }
      return sample;
    },
  };
}

export function findOfficialTooltipElement(trigger: HTMLElement | null): HTMLElement | null {
  if (!trigger?.isConnected) return null;
  const ids = [trigger, trigger.parentElement]
    .flatMap((element) => element?.getAttribute("aria-describedby")?.split(/\s+/) ?? [])
    .filter(Boolean);
  for (const id of ids) {
    const tip = trigger.ownerDocument.getElementById(id);
    if (
      tip?.isConnected &&
      tip.getAttribute("role") === "tooltip" &&
      !tip.hasAttribute("data-incodex-tooltip") &&
      tip.className.trim()
    ) return tip;
  }
  return null;
}

export function nativeTooltipTitle(label: string, shortcut: string): string {
  return shortcut ? `${label} (${shortcut})` : label;
}
