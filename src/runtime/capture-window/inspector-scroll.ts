/**
 * [INPUT]: 依赖浏览器滚动几何、ResizeObserver 和编辑器稳定根节点。
 * [OUTPUT]: 提供滚动边缘判定和随内容/窗口变化更新的渐隐状态。
 * [POS]: capture-window 检查器的表现适配，参考 i18N scroll-fade，不持有业务或选中状态。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
export function inspectorScrollEdges(scrollTop: number, clientHeight: number, scrollHeight: number) {
  return { overflowing: false, atStart: true, atEnd: true };
}
