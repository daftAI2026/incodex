/**
 * [INPUT]: 依赖 AppKit 屏幕几何与 Quartz 全局坐标。
 * [OUTPUT]: 提供 presenter 尺寸、拖放点坐标转换与缓存图片。
 * [POS]: 几何层不持有窗口；Presenter 用转换点命中只读 Settings 窗口。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import AppKit
import Foundation

// Presenter geometry stays independent from panel ownership and animation.
let permissionHostInitialWidth: CGFloat = 600
let permissionHostHelperWidth: CGFloat = 531
let permissionHostHelperHeight: CGFloat = 110
let permissionHostRowWidth: CGFloat = 459
let permissionHostRowHeight: CGFloat = 42
let permissionHostArrowWindowSize: CGFloat = 100
let permissionHostArrowWindowX: CGFloat = 31
let permissionHostArrowWindowY: CGFloat = 60
let permissionHostArrowGraphicSize: CGFloat = 28

func permissionHostCachedImage(_ view: NSView) -> NSImage? {
    view.layoutSubtreeIfNeeded()
    let bounds = view.bounds
    guard bounds.width > 0, bounds.height > 0,
          let representation = view.bitmapImageRepForCachingDisplay(in: bounds) else { return nil }
    view.displayIfNeeded()
    view.cacheDisplay(in: bounds, to: representation)
    let image = NSImage(size: bounds.size)
    image.addRepresentation(representation)
    return image
}

func permissionHostScreenFrame(_ view: NSView) -> NSRect {
    guard let window = view.window else { return view.bounds }
    let windowRect = view.convert(view.bounds, to: nil)
    return window.convertToScreen(windowRect)
}

func permissionHostIntegral(_ value: CGFloat) -> CGFloat { floor(value) }

func permissionHostQuartzPoint(fromAppKit point: NSPoint) -> NSPoint? {
    guard let screen = NSScreen.screens.first,
          point.x.isFinite, point.y.isFinite,
          screen.frame.maxY.isFinite else { return nil }
    return NSPoint(x: point.x, y: screen.frame.maxY - point.y)
}

func permissionHostFrame(_ target: NSRect, size: NSSize) -> NSRect? {
    guard let screen = NSScreen.screens.first else { return nil }
    let screenFrame = screen.frame
    guard target.origin.x.isFinite, target.origin.y.isFinite,
          target.width.isFinite, target.width > 0,
          target.height.isFinite, target.height > 0,
          size.width.isFinite, size.width > 0,
          size.height.isFinite, size.height > 0,
          screenFrame.origin.x.isFinite, screenFrame.origin.y.isFinite,
          screenFrame.width.isFinite, screenFrame.width > 0,
          screenFrame.height.isFinite, screenFrame.height > 0 else { return nil }

    let x = target.minX + target.width - size.width - 10
    let y = screenFrame.minY + screenFrame.height - target.minY - target.height + 10
    let maxX = x + size.width
    let maxY = y + size.height
    guard x.isFinite, y.isFinite, maxX.isFinite, maxY.isFinite else { return nil }
    let left = permissionHostIntegral(x)
    let bottom = permissionHostIntegral(y)
    let width = ceil(maxX) - left
    let height = ceil(maxY) - bottom
    guard left.isFinite, bottom.isFinite,
          width.isFinite, width > 0,
          height.isFinite, height > 0 else { return nil }
    return NSRect(x: left, y: bottom, width: width, height: height)
}
