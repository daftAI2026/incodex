/**
 * [INPUT]: 依赖浏览器滚动几何、ResizeObserver 和编辑器稳定根节点。
 * [OUTPUT]: 提供滚动边缘判定和随内容/窗口变化更新的渐隐状态。
 * [POS]: capture-window 检查器的表现适配，参考 i18N scroll-fade，不持有业务或选中状态。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
const EDGE_TOLERANCE = 1;

export function inspectorScrollEdges(scrollTop: number, clientHeight: number, scrollHeight: number) {
  const maxScroll = Math.max(0, scrollHeight - clientHeight);
  const overflowing = maxScroll > EDGE_TOLERANCE;
  return {
    overflowing,
    atStart: !overflowing || scrollTop <= EDGE_TOLERANCE,
    atEnd: !overflowing || scrollTop >= maxScroll - EDGE_TOLERANCE,
  };
}

export function createInspectorScroll(root: HTMLElement) {
  let viewport: HTMLElement | null = null;
  const sync = () => {
    if (!viewport) return;
    const edges = inspectorScrollEdges(viewport.scrollTop, viewport.clientHeight, viewport.scrollHeight);
    for (const [key, value] of Object.entries(edges)) {
      const next = String(value);
      if (viewport.dataset[key] !== next) viewport.dataset[key] = next;
    }
  };
  const observer = new ResizeObserver(sync);
  const onScroll = (event: Event) => {
    if (event.target === viewport) sync();
  };
  root.addEventListener("scroll", onScroll, true);
  return {
    refresh: () => {
      observer.disconnect();
      viewport = root.querySelector<HTMLElement>(".incodex-capture-inspector-scroll");
      if (viewport) {
        observer.observe(viewport);
        if (viewport.firstElementChild) observer.observe(viewport.firstElementChild);
      }
      sync();
    },
    destroy: () => {
      observer.disconnect();
      root.removeEventListener("scroll", onScroll, true);
      viewport = null;
    },
  };
}
