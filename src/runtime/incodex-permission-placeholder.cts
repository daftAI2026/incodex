// @ts-nocheck
// CUA 1001067 InProgressPermissionRowButtonStyle stores isHovered.
// Normal/hover/pressed stroke: .16/.18/.22; hover fill: .018.
function createPermissionPlaceholderBackground({ quartz, foundation, kit, graphics, size, hovered = false, pressed = false }) {
  const layer = quartz.CAShapeLayer.layer();
  layer.setFrame$({ origin: { x: 0, y: 0 }, size });
  layer.setLineWidth$(1);
  const dash = foundation.NSMutableArray.array();
  for (const length of [3, 6]) dash.addObject$(foundation.NSNumber.numberWithDouble$(length));
  layer.setLineDashPattern$(dash);
  updatePermissionPlaceholderState({ layer, kit, graphics, hovered, pressed });
  graphics.setPlaceholderPath(layer);
  return layer;
}
function updatePermissionPlaceholderState({ layer, kit, graphics, hovered = false, pressed = false }) {
  const color = kit.NSColor.labelColor().colorUsingColorSpace$(kit.NSColorSpace.deviceRGBColorSpace());
  const rgb = [Number(color.redComponent()), Number(color.greenComponent()), Number(color.blueComponent())];
  graphics.setColor(layer, "strokeColor", [...rgb, pressed ? .22 : hovered ? .18 : .16]);
  graphics.setColor(layer, "fillColor", [...rgb, hovered ? .018 : 0]);
}
export { createPermissionPlaceholderBackground, updatePermissionPlaceholderState };
