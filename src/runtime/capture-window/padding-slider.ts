/**
 * [INPUT]: 原生 range 控件及编辑器预览/提交回调。
 * [OUTPUT]: 提供 padding 单滑块的键盘与提交事务适配。
 * [POS]: capture-window 输入边界，保留浏览器指针行为，不持有背景业务状态。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
const LARGE_STEP = 10;

export function wirePaddingSlider(
  input: HTMLInputElement,
  preview: (value: number) => void,
  commit: (value: number) => void,
): void {
  let lastValue = Number(input.value);
  let pending = false;
  const describe = () => input.setAttribute("aria-valuetext", `${input.value}%`);
  const update = () => {
    if (input.disabled) return;
    const value = Number(input.value);
    describe();
    if (!Number.isFinite(value) || value === lastValue) return;
    lastValue = value;
    pending = true;
    preview(value);
  };
  const finish = () => {
    if (!pending) return;
    pending = false;
    commit(lastValue);
  };
  describe();
  input.addEventListener("input", update);
  input.addEventListener("change", () => { update(); finish(); });
  // 原生 range 负责指针捕获；中断保留最后可见值，结束事件不重复持久化。
  input.addEventListener("blur", finish);
  input.addEventListener("pointercancel", finish);
  input.addEventListener("keydown", (event) => {
    if (input.disabled || event.defaultPrevented) return;
    const current = Number(input.value);
    const min = Number(input.min), max = Number(input.max), step = Number(input.step);
    const increment = event.shiftKey ? LARGE_STEP : step;
    const rtl = input.ownerDocument.defaultView?.getComputedStyle(input).direction === "rtl";
    let next: number;
    switch (event.key) {
      case "ArrowRight": next = current + (rtl ? -increment : increment); break;
      case "ArrowLeft": next = current + (rtl ? increment : -increment); break;
      case "ArrowUp": next = current + increment; break;
      case "ArrowDown": next = current - increment; break;
      case "PageUp": next = current + LARGE_STEP; break;
      case "PageDown": next = current - LARGE_STEP; break;
      case "Home": next = min; break;
      case "End": next = max; break;
      default: return;
    }
    event.preventDefault();
    event.stopPropagation();
    input.value = String(Math.min(max, Math.max(min, min + Math.round((next - min) / step) * step)));
    update();
    finish();
  });
}
