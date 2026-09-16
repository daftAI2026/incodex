import { expect, test } from "bun:test";
import { createPermissionCardBackground } from "./incodex-permission-card.cts";

function fixture(dark = false) {
  const objects: any[] = [];
  function view() {
    const values: Record<string, any> = {};
    const item: any = { values, children: [], initWithFrame$: (frame: any) => { values.frame = frame; return item; },
      layer: () => item, addSubview$: (child: any) => item.children.push(child) };
    for (const key of ["WantsLayer", "MasksToBounds", "CornerRadius", "CornerCurve", "ShadowOpacity", "ShadowRadius", "ShadowOffset", "Material", "BlendingMode", "State", "BorderWidth"])
      item[`set${key}$`] = (value: any) => { values[key] = value; };
    objects.push(item); return item;
  }
  const color = { colorUsingColorSpace$: () => color, redComponent: () => 1, greenComponent: () => 1, blueComponent: () => 1, alphaComponent: () => .3 };
  const result = createPermissionCardBackground({
    View: { alloc: view }, Material: { alloc: view }, str: (s: string) => s,
    kit: { NSColor: { separatorColor: () => color }, NSColorSpace: { deviceRGBColorSpace: () => ({}) } },
    graphics: { setBlackColor: (layer: any, key: string, alpha: number) => { layer.values[key] = [0, 0, 0, alpha]; },
      setColor: (layer: any, key: string, rgba: number[]) => { layer.values[key] = rgba; } },
    size: { width: 520, height: 80 }, dark,
  });
  return { objects, result };
}

test("CUA card renders two nested shadows around a clipped continuous native material", () => {
  const { objects, result } = fixture();
  const shadows = objects.filter(x => x.values.ShadowOpacity === 1);
  expect(shadows.map(x => [x.values.shadowColor[3], x.values.ShadowRadius, x.values.ShadowOffset.height]))
    .toEqual([[.2, 3, 0], [.09, 25, 5]]);
  const material = objects.find(x => x.values.Material !== undefined);
  expect(material.values).toMatchObject({ Material: 6, BlendingMode: 1, State: 1,
    CornerRadius: 24, CornerCurve: "continuous", MasksToBounds: true });
  expect(result.children[0].children[0]).toBe(material);
  expect(material.values.BorderWidth).toBe(0);
});

test("only the dark card has the source separator outline, retaining its semantic alpha", () => {
  const { objects } = fixture(true);
  const material = objects.find(x => x.values.Material !== undefined);
  expect(material.values.BorderWidth).toBe(1);
  expect(material.values.borderColor).toEqual([1, 1, 1, .3 * .75]);
});
