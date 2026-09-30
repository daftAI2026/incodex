import AppKit
import Foundation
import SwiftUI

private let copy: NSDictionary = [
    "title": "Enable ChatGPT script control",
    "body": "Installing Incodex modifies ChatGPT, so Accessibility permission must be granted again.",
    "permissionTitle": "Accessibility",
    "permissionDescription": "Read and interact with app interfaces",
    "repair": "Allow",
    "later": "Skip",
    "completeInSettings": "Complete in Settings",
    "back": "Back",
    "dragInstruction": "Drag ChatGPT to the list above to allow Accessibility",
]

private func bitmapRep(_ image: NSImage) -> NSBitmapImageRep {
    if let rep = image.representations.compactMap({ $0 as? NSBitmapImageRep }).first { return rep }
    guard let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
        fatalError("permission-card snapshot did not return rasterizable image content")
    }
    return NSBitmapImageRep(cgImage: cgImage)
}

private func bitmapBytes(_ image: NSImage) -> Data {
    let rep = bitmapRep(image)
    guard let data = rep.bitmapData else { return Data() }
    return Data(bytes: data, count: rep.bytesPerRow * rep.pixelsHigh)
}

private func pixelPoint(_ image: NSImage, _ point: NSPoint) -> (NSBitmapImageRep, Int, Int) {
    let rep = bitmapRep(image)
    let scaleX = CGFloat(rep.pixelsWide) / image.size.width
    let scaleY = CGFloat(rep.pixelsHigh) / image.size.height
    let x = min(rep.pixelsWide - 1, max(0, Int(floor(point.x * scaleX))))
    let y = min(rep.pixelsHigh - 1, max(0, Int(floor(point.y * scaleY))))
    return (rep, x, y)
}

private func alphaAt(_ image: NSImage, _ point: NSPoint) -> CGFloat {
    let (rep, x, y) = pixelPoint(image, point)
    return rep.colorAt(x: x, y: y)?.alphaComponent ?? 0
}

private func alphaPixelCount(_ image: NSImage, rect: NSRect, threshold: CGFloat = 0.1) -> Int {
    let rep = bitmapRep(image)
    let scaleX = CGFloat(rep.pixelsWide) / image.size.width
    let scaleY = CGFloat(rep.pixelsHigh) / image.size.height
    let minX = max(0, Int(floor(rect.minX * scaleX)))
    let maxX = min(rep.pixelsWide, Int(ceil(rect.maxX * scaleX)))
    let minY = max(0, Int(floor(rect.minY * scaleY)))
    let maxY = min(rep.pixelsHigh, Int(ceil(rect.maxY * scaleY)))
    guard minX < maxX, minY < maxY else { return 0 }
    var count = 0
    for y in minY..<maxY {
        for x in minX..<maxX {
            if (rep.colorAt(x: x, y: y)?.alphaComponent ?? 0) > threshold { count += 1 }
        }
    }
    return count
}

private func redPixelCount(_ image: NSImage, rect: NSRect) -> Int {
    let rep = bitmapRep(image)
    let scaleX = CGFloat(rep.pixelsWide) / image.size.width
    let scaleY = CGFloat(rep.pixelsHigh) / image.size.height
    let minX = max(0, Int(floor(rect.minX * scaleX)))
    let maxX = min(rep.pixelsWide, Int(ceil(rect.maxX * scaleX)))
    let minY = max(0, Int(floor(rect.minY * scaleY)))
    let maxY = min(rep.pixelsHigh, Int(ceil(rect.maxY * scaleY)))
    guard minX < maxX, minY < maxY else { return 0 }
    var count = 0
    for y in minY..<maxY {
        for x in minX..<maxX {
            guard let color = rep.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) else { continue }
            if color.alphaComponent > 0.4 && color.redComponent > color.greenComponent + 0.2
                && color.redComponent > color.blueComponent + 0.2 {
                count += 1
            }
        }
    }
    return count
}

private func bluePixelCount(_ image: NSImage, rect: NSRect) -> Int {
    let rep = bitmapRep(image)
    let scaleX = CGFloat(rep.pixelsWide) / image.size.width
    let scaleY = CGFloat(rep.pixelsHigh) / image.size.height
    let minX = max(0, Int(floor(rect.minX * scaleX)))
    let maxX = min(rep.pixelsWide, Int(ceil(rect.maxX * scaleX)))
    let minY = max(0, Int(floor(rect.minY * scaleY)))
    let maxY = min(rep.pixelsHigh, Int(ceil(rect.maxY * scaleY)))
    guard minX < maxX, minY < maxY else { return 0 }
    var count = 0
    for y in minY..<maxY {
        for x in minX..<maxX {
            guard let color = rep.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) else { continue }
            if color.alphaComponent > 0.4 && color.blueComponent > color.redComponent + 0.2
                && color.blueComponent > color.greenComponent + 0.05 {
                count += 1
            }
        }
    }
    return count
}

private func makeIcon() -> NSImage {
    let image = NSImage(size: NSSize(width: 64, height: 64))
    image.lockFocus()
    NSColor.red.setFill()
    NSBezierPath(roundedRect: NSRect(x: 0, y: 0, width: 64, height: 64), xRadius: 14, yRadius: 14).fill()
    image.unlockFocus()
    return image
}

private func assertSnapshotScale(_ image: NSImage, scale: CGFloat) {
    precondition(image.size == NSSize(width: 518, height: 80), "permission-card snapshot logical size changed: \(image.size)")
    let rep = bitmapRep(image)
    precondition(rep.pixelsWide == Int(518 * scale), "permission-card snapshot width lost backing scale")
    precondition(rep.pixelsHigh == Int(80 * scale), "permission-card snapshot height lost backing scale")
    precondition(!bitmapBytes(image).isEmpty, "permission-card snapshot is empty")
}

private func assertRect(_ actual: NSRect, _ expected: NSRect, _ message: String) {
    precondition(abs(actual.origin.x - expected.origin.x) <= 0.01
                 && abs(actual.origin.y - expected.origin.y) <= 0.01
                 && abs(actual.size.width - expected.size.width) <= 0.01
                 && abs(actual.size.height - expected.size.height) <= 0.01,
                 "\(message) changed: before=\(expected), after=\(actual)")
}

private struct LiveGeometry {
    let viewBounds: NSRect
    let cardFrame: NSRect
    let cardBounds: NSRect
    let card: NSView
}

@MainActor
private func geometry(of view: IncodexPermissionInitialView) -> LiveGeometry {
    view.setFrameSize(view.preferredContentSize)
    view.layoutSubtreeIfNeeded()
    for subview in view.subviews { subview.layoutSubtreeIfNeeded() }
    let card = view.permissionCardView
    card.layoutSubtreeIfNeeded()
    return LiveGeometry(
        viewBounds: view.bounds,
        cardFrame: card.convert(card.bounds, to: view),
        cardBounds: card.bounds,
        card: card,
    )
}

private func assertCardForeground(_ image: NSImage) {
    // The new capture is the foreground content only. These points are away
    // from the icon, text, and Allow button and therefore must remain clear;
    // a material, clip shell, or card shadow would make this fail.
    for point in [NSPoint(x: 258, y: 1), NSPoint(x: 76, y: 40), NSPoint(x: 350, y: 78)] {
        precondition(alphaAt(image, point) == 0, "card foreground is not transparent at \(point)")
    }

    let iconRect = NSRect(x: 8, y: 8, width: 64, height: 64)
    let textRect = NSRect(x: 84, y: 10, width: 300, height: 60)
    let buttonRect = NSRect(x: 400, y: 8, width: 110, height: 64)
    precondition(redPixelCount(image, rect: iconRect) > 500, "permission icon was omitted from card foreground")
    precondition(alphaPixelCount(image, rect: textRect, threshold: 0.1) > 40, "permission title/description was omitted")
    precondition(alphaPixelCount(image, rect: buttonRect, threshold: 0.1) > 40, "Allow control was omitted from card foreground")
    precondition(bluePixelCount(image, rect: buttonRect) > 100,
                 "Allow snapshot lost its blue default-button fill")
    precondition(redPixelCount(image, rect: buttonRect) == 0,
                 "Allow snapshot contains a forbidden-operation mark")
}

@main
enum PermissionCardSnapshotSmoke {
    @MainActor
    static func main() {
        _ = NSApplication.shared

        let view = IncodexPermissionInitialView(frame: NSRect(x: 0, y: 0, width: 600, height: 340))
        let icon = makeIcon()
        view.configure(copy: copy, appIcon: nil, permissionIcon: icon, actionTarget: nil)
        view.setContent(title: "Initial", body: "Initial body", allowEnabled: true, settingsPlaceholder: false)

        let before = geometry(of: view)
        guard let scale1 = view.snapshotPermissionCard(scale: 1),
              let scale2 = view.snapshotPermissionCard(scale: 2) else {
            fatalError("snapshotPermissionCard(scale:) unexpectedly returned nil")
        }
        assertSnapshotScale(scale1, scale: 1)
        assertSnapshotScale(scale2, scale: 2)
        assertCardForeground(scale2)

        // State changes must be visible immediately. Do not yield to a run
        // loop between configure/setContent and the new API call.
        let updatedCopy = NSMutableDictionary(dictionary: copy)
        updatedCopy["permissionTitle"] = "Updated Accessibility"
        updatedCopy["permissionDescription"] = "Updated permission description"
        view.configure(copy: updatedCopy, appIcon: nil, permissionIcon: icon, actionTarget: nil)
        guard let configured = view.snapshotPermissionCard(scale: 1) else {
            fatalError("configured permission-card snapshot unexpectedly returned nil")
        }
        precondition(bitmapBytes(configured) != bitmapBytes(scale1),
                     "permission-card snapshot did not observe configure state synchronously")
        assertCardForeground(configured)

        view.setContent(title: "Updated", body: "Updated body", allowEnabled: false, settingsPlaceholder: false)
        guard let disabled = view.snapshotPermissionCard(scale: 1) else {
            fatalError("disabled permission-card snapshot unexpectedly returned nil")
        }
        precondition(bitmapBytes(disabled) != bitmapBytes(configured),
                     "permission-card snapshot did not observe setContent state synchronously")

        let after = geometry(of: view)
        assertRect(after.viewBounds, before.viewBounds, "initial view bounds")
        assertRect(after.cardFrame, before.cardFrame, "live permission-card frame")
        assertRect(after.cardBounds, before.cardBounds, "live permission-card bounds")
        precondition(after.card === before.card, "snapshot replaced the live permission-card host")
        precondition(NSApp.windows.allSatisfy { !$0.isVisible }, "permission-card snapshot smoke opened a window")
        print("permission card snapshot smoke passed (windowless; immediate state)")
    }
}
