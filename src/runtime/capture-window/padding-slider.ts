/**
 * [INPUT]: 原生 range 控件及编辑器预览/提交回调。
 * [OUTPUT]: 提供 padding 单滑块的键盘与提交事务适配。
 * [POS]: capture-window 输入边界，保留浏览器指针行为，不持有背景业务状态。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
export function wirePaddingSlider(input: HTMLInputElement, preview: (value: number) => void, commit: (value: number) => void): void {}
