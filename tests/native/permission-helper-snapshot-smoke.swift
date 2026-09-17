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

        let helper = IncodexPermissionHelperView(frame: NSRect(x: 0, y: 0, width: 531, height: 110))
        helper.appearance = NSAppearance(named: .aqua)
        helper.configure(copy: shortCopy, appIcon: makeAppIcon(), actionTarget: nil)
        helper.layoutSubtreeIfNeeded()
        let host = helper.subviews.first
        let rowHost = helper.appRowView

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
