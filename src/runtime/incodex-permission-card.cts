// @ts-nocheck
// PermissionRow from CUA build 1001067: 24pt continuous corners, regular material,
// two nested shadows, and a separator outline only in the dark color scheme.
// A local SwiftUI/AppKit probe verifies Popover's regular-material color layers.
function createPermissionCardBackground({ View, Material, kit, graphics, str, size, dark }) {
  const bounds = { origin: { x: 0, y: 0 }, size };
  function shadow(alpha, radius, y) {
    const view = View.alloc().initWithFrame$(bounds);
    view.setWantsLayer$(true);
    const layer = view.layer();
    layer.setMasksToBounds$(false);
    graphics.setBlackColor(layer, "shadowColor", alpha);
    layer.setShadowOpacity$(1); layer.setShadowRadius$(radius);
    layer.setShadowOffset$({ width: 0, height: y });
    // No shadowPath: derive the continuous silhouette from the clipped material,
    // matching SwiftUI's nested layers rather than substituting circular paths.
    return view;
  }
  const outer = shadow(.2, 3, 0);
  const inner = shadow(.09, 25, 5);
  const material = Material.alloc().initWithFrame$(bounds);
  material.setMaterial$(6); material.setBlendingMode$(1); material.setState$(1);
  material.setWantsLayer$(true);
  const layer = material.layer();
  layer.setCornerRadius$(24); layer.setCornerCurve$(str("continuous"));
  layer.setMasksToBounds$(true); layer.setBorderWidth$(dark ? 1 : 0);
  if (dark) {
    const color = kit.NSColor.separatorColor().colorUsingColorSpace$(kit.NSColorSpace.deviceRGBColorSpace());
    graphics.setColor(layer, "borderColor", [Number(color.redComponent()), Number(color.greenComponent()),
      Number(color.blueComponent()), Number(color.alphaComponent()) * .75]);
  }
  inner.addSubview$(material); outer.addSubview$(inner);
  return outer;
}
export { createPermissionCardBackground };
