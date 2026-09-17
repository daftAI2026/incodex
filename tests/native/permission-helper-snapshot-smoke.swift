import AppKit
import Foundation
import SwiftUI

private let shortCopy: NSDictionary = [
    "title": "Enable ChatGPT script control",
    "body": "Installing Incodex modifies ChatGPT, so Accessibility permission must be granted again.",
    "permissionTitle": "Accessibility",
    "permissionDescription": "Read and interact with app interfaces",
    "repair": "Allow",
    "later": "Skip",
    "back": "Back",
    "dragInstruction": "Drag ChatGPT to the list above to allow Accessibility",
    "dragInstructionRuns": "[{\"text\":\"Drag \",\"role\":\"secondary\"},{\"text\":\"ChatGPT\",\"role\":\"primary\"},{\"text\":\" to the list above to allow \",\"role\":\"secondary\"},{\"text\":\"Accessibility\",\"role\":\"primary\"}]",
]

private let longCopy: NSDictionary = [
    "title": "ChatGPT-Skriptsteuerung aktivieren",
    "body": "Durch die Installation von Incodex wird ChatGPT geändert. Daher muss die Bedienungshilfen-Berechtigung erneut erteilt werden.",
    "permissionTitle": "Bedienungshilfen",
    "permissionDescription": "App-Oberflächen lesen und mit ihnen interagieren",
    "repair": "Erlauben",
    "later": "Überspringen",
    "back": "Zurück",
    "dragInstruction": "Ziehe ChatGPT in die Liste oben, um den Zugriff auf die Bedienungshilfen zu erlauben",
    "dragInstructionRuns": "[{\"text\":\"Ziehe \",\"role\":\"secondary\"},{\"text\":\"ChatGPT\",\"role\":\"primary\"},{\"text\":\" in die Liste oben, um den Zugriff auf die \",\"role\":\"secondary\"},{\"text\":\"Bedienungshilfen\",\"role\":\"primary\"},{\"text\":\" zu erlauben\",\"role\":\"secondary\"}]",
]

private func bitmapRep(_ image: NSImage) -> NSBitmapImageRep {
    if let rep = image.representations.compactMap({ $0 as? NSBitmapImageRep }).first { return rep }
    guard let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
        fatalError("snapshot did not return rasterizable image content")
    }
    return NSBitmapImageRep(cgImage: cgImage)
}

private func alphaAtTopLeft(_ image: NSImage, point: NSPoint) -> CGFloat {
    let rep = bitmapRep(image)
    let scaleX = CGFloat(rep.pixelsWide) / image.size.width
    let scaleY = CGFloat(rep.pixelsHigh) / image.size.height
    let x = min(rep.pixelsWide - 1, max(0, Int(floor(point.x * scaleX))))
    let yFromTop = min(rep.pixelsHigh - 1, max(0, Int(floor(point.y * scaleY))))
    return rep.colorAt(x: x, y: yFromTop)?.alphaComponent ?? 0
}

private func bitmapBytes(_ image: NSImage) -> Data {
    let rep = bitmapRep(image)
    guard let data = rep.bitmapData else { return Data() }
    return Data(bytes: data, count: rep.bytesPerRow * rep.pixelsHigh)
}

private func assertScale(_ image: NSImage, scale: CGFloat) {
    precondition(image.size == NSSize(width: 531, height: 110), "logical helper snapshot size changed")
    let rep = bitmapRep(image)
    precondition(rep.pixelsWide == Int(531 * scale), "snapshot width did not preserve backing scale")
    precondition(rep.pixelsHigh == Int(110 * scale), "snapshot height did not preserve backing scale")
}

private func assertInstructionIsRendered(_ image: NSImage) {
    let rep = bitmapRep(image)
    let scale = CGFloat(rep.pixelsWide) / image.size.width
    var inkPixels = 0
    // The instruction sits above the row on a transparent background. A
    // nonempty row fill alone must not pass a missing-SwiftUI-text regression.
    for y in Int(17 * scale)..<Int(35 * scale) {
        for x in Int(102 * scale)..<Int(510 * scale) {
            if (rep.colorAt(x: x, y: y)?.alphaComponent ?? 0) > 0.1 { inkPixels += 1 }
        }
    }
    precondition(inkPixels > 100, "snapshot omitted the instruction text")
}

private func firstNSBoxAncestor(of view: NSView) -> NSBox? {
    var ancestor = view.superview
    while let current = ancestor {
        if let box = current as? NSBox { return box }
        ancestor = current.superview
    }
    return nil
}

private func assertNearlyEqual(_ actual: CGFloat, _ expected: CGFloat, tolerance: CGFloat, _ message: String) {
    precondition(abs(actual - expected) <= tolerance, "\(message): expected \(expected), got \(actual)")
}

private func assertRect(_ actual: NSRect, _ expected: NSRect, tolerance: CGFloat, _ message: String) {
    assertNearlyEqual(actual.origin.x, expected.origin.x, tolerance: tolerance, "\(message).origin.x")
    assertNearlyEqual(actual.origin.y, expected.origin.y, tolerance: tolerance, "\(message).origin.y")
    assertNearlyEqual(actual.size.width, expected.size.width, tolerance: tolerance, "\(message).width")
    assertNearlyEqual(actual.size.height, expected.size.height, tolerance: tolerance, "\(message).height")
}

private func resolvedRGBA(_ color: NSColor, in appearance: NSAppearance) -> (CGFloat, CGFloat, CGFloat, CGFloat) {
    var result = (CGFloat.zero, CGFloat.zero, CGFloat.zero, CGFloat.zero)
    appearance.performAsCurrentDrawingAppearance {
        guard let rgb = color.usingColorSpace(.sRGB) else {
            fatalError("color did not convert to device RGB")
        }
        result = (rgb.redComponent, rgb.greenComponent, rgb.blueComponent, rgb.alphaComponent)
    }
    return result
}

private func assertPermissionRowBoxContract(_ helper: IncodexPermissionHelperView, rowHost: NSView) {
    let hostingType = String(describing: type(of: rowHost))
    precondition(hostingType.contains("NSHostingView"), "helper row must remain an NSHostingView: \(hostingType)")
    guard let box = firstNSBoxAncestor(of: rowHost) else {
        fatalError("helper row must be wrapped by an ordinary NSBox")
    }
    guard let contentView = box.contentView else {
        fatalError("helper row NSBox has no contentView")
    }

    precondition(contentView === rowHost, "helper row NSBox contentView must be the live NSHostingView")
    assertRect(box.bounds, NSRect(x: 0, y: 0, width: 459, height: 42), tolerance: 0.1, "helper row box bounds")
    precondition(box.boxType == .custom, "helper row NSBox must use custom boxType")
    precondition(box.titlePosition == .noTitle, "helper row NSBox must have no title")
    assertNearlyEqual(box.cornerRadius, 7, tolerance: 0.1, "helper row box cornerRadius")
    assertNearlyEqual(box.contentViewMargins.width, 4, tolerance: 0.1, "helper row box margins.width")
    assertNearlyEqual(box.contentViewMargins.height, 5, tolerance: 0.1, "helper row box margins.height")
    assertNearlyEqual(box.borderWidth, 1, tolerance: 0.1, "helper row box borderWidth")
    assertRect(contentView.frame, NSRect(x: 5, y: 6, width: 449, height: 30), tolerance: 0.1, "helper row contentView frame")

    helper.appearance = NSAppearance(named: .aqua)
    helper.layoutSubtreeIfNeeded()
    let aqua = NSAppearance(named: .aqua)!
    let aquaFill = resolvedRGBA(box.fillColor, in: aqua)
    assertNearlyEqual(aquaFill.0, 1, tolerance: 0.03, "Aqua row fill.red")
    assertNearlyEqual(aquaFill.1, 1, tolerance: 0.03, "Aqua row fill.green")
    assertNearlyEqual(aquaFill.2, 1, tolerance: 0.03, "Aqua row fill.blue")
    assertNearlyEqual(aquaFill.3, 0.65, tolerance: 0.03, "Aqua row fill.alpha")
    let aquaBorder = resolvedRGBA(box.borderColor, in: aqua)
    assertNearlyEqual(aquaBorder.0, 223 / 255, tolerance: 0.03, "Aqua row border.red")
    assertNearlyEqual(aquaBorder.1, 221 / 255, tolerance: 0.03, "Aqua row border.green")
    assertNearlyEqual(aquaBorder.2, 227 / 255, tolerance: 0.03, "Aqua row border.blue")
    assertNearlyEqual(aquaBorder.3, 1, tolerance: 0.03, "Aqua row border.alpha")

    helper.appearance = NSAppearance(named: .darkAqua)
    helper.layoutSubtreeIfNeeded()
    let dark = NSAppearance(named: .darkAqua)!
    let darkFill = resolvedRGBA(box.fillColor, in: dark)
    assertNearlyEqual(darkFill.0, 0, tolerance: 0.03, "Dark row fill.red")
    assertNearlyEqual(darkFill.1, 0, tolerance: 0.03, "Dark row fill.green")
    assertNearlyEqual(darkFill.2, 0, tolerance: 0.03, "Dark row fill.blue")
    assertNearlyEqual(darkFill.3, 0.06, tolerance: 0.03, "Dark row fill.alpha")
    var expectedDarkBorder = (CGFloat.zero, CGFloat.zero, CGFloat.zero, CGFloat.zero)
    // withAlphaComponent may resolve the semantic color immediately, before
    // usingColorSpace. Construct it under the same appearance as AppKit does.
    dark.performAsCurrentDrawingAppearance {
        expectedDarkBorder = resolvedRGBA(NSColor.textColor.withAlphaComponent(0.08), in: dark)
    }
    let darkBorder = resolvedRGBA(box.borderColor, in: dark)
    assertNearlyEqual(darkBorder.0, expectedDarkBorder.0, tolerance: 0.03, "Dark row border.red")
    assertNearlyEqual(darkBorder.1, expectedDarkBorder.1, tolerance: 0.03, "Dark row border.green")
    assertNearlyEqual(darkBorder.2, expectedDarkBorder.2, tolerance: 0.03, "Dark row border.blue")
    assertNearlyEqual(darkBorder.3, 0.08, tolerance: 0.03, "Dark row border.alpha")

    rowHost.isHidden = true
    precondition(!box.isHidden, "hiding the row host must not hide the NSBox")
    rowHost.isHidden = false
    precondition(firstNSBoxAncestor(of: rowHost) === box, "row host lost its NSBox ancestor after unhide")
    precondition(box.contentView === rowHost, "row host identity changed after unhide")
    helper.appearance = NSAppearance(named: .aqua)
    helper.layoutSubtreeIfNeeded()
}

private func makeAppIcon() -> NSImage {
    let image = NSImage(size: NSSize(width: 32, height: 32))
    image.lockFocus()
    NSColor.systemBlue.setFill()
    NSBezierPath(roundedRect: NSRect(x: 0, y: 0, width: 32, height: 32), xRadius: 7, yRadius: 7).fill()
    image.unlockFocus()
    return image
}

@main
enum PermissionHelperSnapshotSmoke {
    @MainActor
    static func main() {
        _ = NSApplication.shared

        let sharedIcon = makeAppIcon()
        sharedIcon.size = NSSize(width: 64, height: 64)
        guard let rowIcon = permissionHelperAppIcon(sharedIcon) else { fatalError("missing helper icon") }
        precondition(rowIcon !== sharedIcon, "helper must not resize the initial window's shared icon")
        precondition(rowIcon.size == NSSize(width: 32, height: 32), "helper icon intrinsic size must be 32pt")
        precondition(sharedIcon.size == NSSize(width: 64, height: 64), "shared initial icon size was mutated")
        precondition(permissionHelperAppIcon(nil) == nil)

        let helper = IncodexPermissionHelperView(frame: NSRect(x: 0, y: 0, width: 531, height: 110))
        helper.appearance = NSAppearance(named: .aqua)
        helper.configure(copy: shortCopy, appIcon: makeAppIcon(), actionTarget: nil)
        helper.layoutSubtreeIfNeeded()
        let host = helper.subviews.first
        let rowHost = helper.appRowView
        assertPermissionRowBoxContract(helper, rowHost: rowHost)

        guard let light1 = helper.snapshotImage(scale: 1) else {
            fatalError("scale 1 snapshot unexpectedly returned nil")
        }
        guard let light2 = helper.snapshotImage(scale: 2) else {
            fatalError("scale 2 snapshot unexpectedly returned nil")
        }
        assertScale(light1, scale: 1)
        assertScale(light2, scale: 2)
        assertInstructionIsRendered(light1)
        assertInstructionIsRendered(light2)
        precondition(alphaAtTopLeft(light2, point: NSPoint(x: 500, y: 5)) == 0, "snapshot background is not transparent")
        precondition(alphaAtTopLeft(light2, point: NSPoint(x: 250, y: 65)) > 0, "snapshot row has no foreground output")
        precondition(!bitmapBytes(light2).isEmpty, "snapshot is empty")
        precondition(helper.subviews.first === host, "snapshot replaced the live helper host")
        precondition(helper.appRowView === rowHost, "snapshot replaced the live drag row host")

        helper.appearance = NSAppearance(named: .darkAqua)
        guard let dark = helper.snapshotImage(scale: 2) else {
            fatalError("dark snapshot unexpectedly returned nil")
        }
        assertScale(dark, scale: 2)
        assertInstructionIsRendered(dark)
        precondition(bitmapBytes(light2) != bitmapBytes(dark), "light and dark snapshots are identical")
        precondition(helper.subviews.first === host, "dark snapshot replaced the live helper host")
        precondition(helper.appRowView === rowHost, "dark snapshot replaced the live drag row host")

        helper.configure(copy: longCopy, appIcon: makeAppIcon(), actionTarget: nil)
        helper.layoutSubtreeIfNeeded()
        guard let long = helper.snapshotImage(scale: 2) else {
            fatalError("long instruction snapshot unexpectedly returned nil")
        }
        precondition(long.size.width == 531 && long.size.height > 110, "long instruction did not expand snapshot")

        helper.configure(copy: shortCopy, appIcon: makeAppIcon(), actionTarget: nil)
        helper.layoutSubtreeIfNeeded()
        guard let shortAfterLong = helper.snapshotImage(scale: 2) else {
            fatalError("short instruction snapshot after long unexpectedly returned nil")
        }
        assertScale(shortAfterLong, scale: 2)
        precondition(helper.appRowView === rowHost, "long-to-short reconfiguration replaced the live drag row host")

        for invalidScale in [CGFloat(0), -1, .nan, .infinity] {
            precondition(helper.snapshotImage(scale: invalidScale) == nil, "invalid scale was accepted: \(invalidScale)")
        }

        precondition(NSApp.windows.allSatisfy { !$0.isVisible }, "headless snapshot smoke must not show windows")
        print("permission helper snapshot smoke passed")
    }
}
