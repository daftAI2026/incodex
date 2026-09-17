// @ts-nocheck
// CUA 1001067, PendingPermissionButtonStyle: 1pt stroke, [3,6] dash,
// primary color at .16 (light) / .18 (dark), no card shadows.
function createPermissionPlaceholderBackground({ quartz, foundation, kit, graphics, size, dark }) {
  const layer = quartz.CAShapeLayer.layer();
  layer.setFrame$({ origin: { x: 0, y: 0 }, size });
  layer.setLineWidth$(1);
  const dash = foundation.NSMutableArray.array();
  for (const length of [3, 6]) dash.addObject$(foundation.NSNumber.numberWithDouble$(length));
  layer.setLineDashPattern$(dash);
  const color = kit.NSColor.labelColor().colorUsingColorSpace$(kit.NSColorSpace.deviceRGBColorSpace());
  const rgb = [Number(color.redComponent()), Number(color.greenComponent()), Number(color.blueComponent())];
  graphics.setColor(layer, "strokeColor", [...rgb, dark ? .18 : .16]);
  graphics.setColor(layer, "fillColor", [...rgb, dark ? .018 : 0]);
  graphics.setPlaceholderPath(layer);
  return layer;
}
export { createPermissionPlaceholderBackground };
