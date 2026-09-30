import { expect, test } from "bun:test";
import { createPermissionPlaceholderBackground } from "./incodex-permission-placeholder.cts";

function renderPlaceholder(hovered = false, pressed = false) {
  const layers: any[] = [];
  const native = () => {
    const values: any = {}, layer: any = { values };
    for (const key of ["Frame", "LineWidth", "LineDashPattern", "FillColor", "StrokeColor"]) layer[`set${key}$`] = (v: any) => {values[key] = v;};
    layers.push(layer);return layer;
  };
  const color: any = { colorUsingColorSpace$: () => color, redComponent: () => 0, greenComponent: () => 0, blueComponent: () => 0, alphaComponent: () => 1 };
  const result = createPermissionPlaceholderBackground({
    quartz: { CAShapeLayer: { layer: native } },
    foundation: { NSMutableArray: { array: () => ({items: [] as any[], addObject$(x: any) {this.items.push(x);} }) }, NSNumber: { numberWithDouble$: (x: number) => x } },
    kit: { NSColor: { labelColor: () => color }, NSColorSpace: { deviceRGBColorSpace: () => ({}) } },
    graphics: { setColor: (l: any,k: string,v: any) => l.values[k]=v, setPlaceholderPath: (l: any) => {l.values.path="SwiftUI continuous 518x80 r24";} },
    size: {width:518,height:80}, hovered, pressed,
  });
  return result;
}

test("Settings placeholder uses the reference dashed outline without card shadows", () => {
  const result = renderPlaceholder();
  expect(result.values.LineWidth).toBe(1);
  expect(result.values.LineDashPattern.items).toEqual([3,6]);
  expect(result.values.path).toBe("SwiftUI continuous 518x80 r24");
  expect(result.values.strokeColor).toEqual([0,0,0,.16]);
  expect(result.values.fillColor).toEqual([0,0,0,0]);
  expect(result.values.ShadowOpacity).toBeUndefined();
});


test("placeholder feedback follows hover and press rather than color scheme", () => {
  expect(renderPlaceholder(true).values.strokeColor).toEqual([0,0,0,.18]);
  expect(renderPlaceholder(true).values.fillColor).toEqual([0,0,0,.018]);
  expect(renderPlaceholder(true, true).values.strokeColor).toEqual([0,0,0,.22]);
  expect(renderPlaceholder(false, true).values.strokeColor).toEqual([0,0,0,.22]);
  expect(renderPlaceholder(false, true).values.fillColor).toEqual([0,0,0,0]);
});
