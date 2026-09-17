import { expect, test } from "bun:test";
import { createPermissionPlaceholderBackground } from "./incodex-permission-placeholder.cts";

test("Settings placeholder uses the reference dashed outline without card shadows", () => {
  const layers: any[] = [];
  const native = () => {
    const values: any = {}, layer: any = { values };
    for (const key of ["Frame", "LineWidth", "LineDashPattern", "FillColor", "StrokeColor"]) layer[`set${key}$`] = (v: any) => {values[key] = v;};
    layers.push(layer);return layer;
  };
  const color: any = { colorUsingColorSpace$: () => color, redComponent: () => 0, greenComponent: () => 0, blueComponent: () => 0, alphaComponent: () => 1 };
  const result = createPermissionPlaceholderBackground({
    quartz: { CAShapeLayer: { layer: native } },
    foundation: { NSMutableArray: { array: () => ({items: [], addObject$(x: any) {this.items.push(x);} }) }, NSNumber: { numberWithDouble$: (x: number) => x } },
    kit: { NSColor: { labelColor: () => color }, NSColorSpace: { deviceRGBColorSpace: () => ({}) } },
    graphics: { setColor: (l: any,k: string,v: any) => l.values[k]=v, setRoundedPath: (l: any,b: any,r: number) => {l.values.path={bounds:b,radius:r};} },
    size: {width:518,height:80}, dark:false,
  });
  expect(result.values.LineWidth).toBe(1);
  expect(result.values.LineDashPattern.items).toEqual([3,6]);
  expect(result.values.path.radius).toBe(24);
  expect(result.values.strokeColor).toEqual([0,0,0,.16]);
  expect(result.values.fillColor).toEqual([0,0,0,0]);
  expect(result.values.ShadowOpacity).toBeUndefined();
});
