/**
 * [INPUT]: padding-slider 的原生事件适配与可派发事件的输入替身。
 * [OUTPUT]: 验证预览/提交分离、键盘步长、禁用、边界与结束去重。
 * [POS]: capture-window 单滑块交互回归，不用 CSS 断言冒充输入行为验证。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { wirePaddingSlider } from "./padding-slider.ts";

class Input extends EventTarget {
  value = "8";
  min = "0";
  max = "45";
  step = "1";
  disabled = false;
  attributes = new Map<string, string>();
  direction = "ltr";
  ownerDocument = { defaultView: { getComputedStyle: () => ({ direction: this.direction }) } };
  setAttribute(key: string, value: string) { this.attributes.set(key, value); }
  fire(type: string, props = {}) {
    const event = Object.assign(new Event(type, { cancelable: true }), props);
    this.dispatchEvent(event);
    return event;
  }
}
function setup() {
  const input = new Input();
  const previews: number[] = [], commits: number[] = [];
  wirePaddingSlider(input as unknown as HTMLInputElement, value => previews.push(value), value => commits.push(value));
  return { input, previews, commits };
}
test("native drag previews live and commits once, including interruption", () => {
  const { input, previews, commits } = setup();
  input.value = "10"; input.fire("input"); input.fire("input");
  input.value = "11"; input.fire("input");
  expect(previews).toEqual([10, 11]); expect(commits).toEqual([]);
  input.fire("change"); input.fire("blur");
  expect(commits).toEqual([11]);
  input.value = "12"; input.fire("input"); input.fire("pointercancel"); input.fire("change");
  expect(commits).toEqual([11, 12]);
  expect(input.attributes.get("aria-valuetext")).toBe("12%");
});
test("Base UI single-thumb keyboard semantics use step and largeStep with endpoint clamping", () => {
  const { input, commits } = setup();
  input.fire("keydown", { key: "ArrowRight", shiftKey: true });
  input.fire("keydown", { key: "PageUp" });
  input.fire("keydown", { key: "End" });
  input.fire("keydown", { key: "ArrowUp" });
  input.fire("keydown", { key: "Home" });
  input.fire("keydown", { key: "ArrowRight" });
  input.fire("change");
  expect(commits).toEqual([18, 28, 45, 0, 1]);
});
test("RTL reverses horizontal keys; disabled and unrelated keys remain inert", () => {
  const { input, commits } = setup(); input.direction = "rtl";
  input.fire("keydown", { key: "ArrowRight" });
  expect(commits).toEqual([7]);
  expect(input.fire("keydown", { key: "Tab" }).defaultPrevented).toBe(false);
  input.disabled = true; input.fire("keydown", { key: "End" });
  input.value = "40"; input.fire("input"); input.fire("change");
  expect(commits).toEqual([7]);
});
