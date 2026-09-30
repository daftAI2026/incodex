import AppKit
import Foundation
import SwiftUI

private struct CardCopy {
    let name: String
    let values: [String: String]
    let expectedTextBands: Int
}

private func bitmapRep(_ image: NSImage) -> NSBitmapImageRep {
    if let rep = image.representations.compactMap({ $0 as? NSBitmapImageRep }).first { return rep }
    guard let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
        fatalError("permission-card text snapshot is not rasterizable")
    }
    return NSBitmapImageRep(cgImage: cgImage)
}

private struct PixelBounds {
    var minX = Int.max
    var maxX = Int.min
    var minY = Int.max
    var maxY = Int.min
    var count = 0

    var isEmpty: Bool { count == 0 }

    mutating func include(x: Int, y: Int) {
        minX = min(minX, x)
        maxX = max(maxX, x)
        minY = min(minY, y)
        maxY = max(maxY, y)
        count += 1
    }
}

private func color(_ rep: NSBitmapImageRep, x: Int, y: Int) -> NSColor? {
    rep.colorAt(x: x, y: y)?.usingColorSpace(.sRGB)
}

private func foregroundGeometry(_ image: NSImage) -> (text: PixelBounds, allow: PixelBounds, textBands: [ClosedRange<Int>]) {
    let rep = bitmapRep(image)
    let scaleX = CGFloat(rep.pixelsWide) / image.size.width
    let scaleY = CGFloat(rep.pixelsHigh) / image.size.height
    let textStartX = Int(80 * scaleX)
    let buttonSearchStartX = Int(390 * scaleX)
    let alphaThreshold: CGFloat = 0.08
    var textBounds = PixelBounds()
    var allowBounds = PixelBounds()
    var occupiedTextRows: [Int] = []

    for y in 0..<rep.pixelsHigh {
        var rowInk = false
        for x in 0..<rep.pixelsWide {
            guard let pixel = color(rep, x: x, y: y), pixel.alphaComponent > alphaThreshold else { continue }
            let isBlueControl = pixel.blueComponent > pixel.redComponent + 0.2
                && pixel.blueComponent > pixel.greenComponent + 0.05
            if x >= textStartX && x < buttonSearchStartX {
                let luminance = (pixel.redComponent + pixel.greenComponent + pixel.blueComponent) / 3
                if luminance < 0.9 && !isBlueControl {
                    textBounds.include(x: x, y: y)
                    rowInk = true
                }
            }
            if x >= textStartX && isBlueControl {
                allowBounds.include(x: x, y: y)
            }
        }
        if rowInk { occupiedTextRows.append(y) }
    }

    var bands: [ClosedRange<Int>] = []
    for y in occupiedTextRows {
        if let last = bands.last, y <= last.upperBound + 2 {
            bands[bands.count - 1] = last.lowerBound...y
        } else {
            bands.append(y...y)
        }
    }
    return (textBounds, allowBounds, bands)
}

@MainActor
private func assertCard(_ copy: CardCopy) {
    let values = NSDictionary(dictionary: copy.values)
    let view = IncodexPermissionInitialView(frame: NSRect(x: 0, y: 0, width: 600, height: 340))
    view.appearance = NSAppearance(named: .aqua)
    view.configure(copy: values, appIcon: nil, permissionIcon: nil, actionTarget: nil)
    view.setContent(
        title: copy.values["title"] ?? "Enable ChatGPT scripting",
        body: copy.values["body"] ?? "Allow Accessibility access.",
        allowEnabled: true,
        settingsPlaceholder: false
    )
    view.setFrameSize(view.preferredContentSize)
    view.layoutSubtreeIfNeeded()
    view.permissionCardView.layoutSubtreeIfNeeded()

    guard let image = view.snapshotPermissionCard(scale: 2) else {
        fatalError("\(copy.name): permission-card snapshot returned nil")
    }
    precondition(image.size == NSSize(width: 518, height: 80), "\(copy.name): ordinary card changed from 518×80pt")
    let rep = bitmapRep(image)
    precondition(rep.pixelsWide == 1036 && rep.pixelsHigh == 160, "\(copy.name): 2× snapshot dimensions changed")
    if let debugDirectory = ProcessInfo.processInfo.environment["INCODEX_CARD_TEXT_DEBUG_DIR"],
       let data = rep.representation(using: .png, properties: [:]) {
        try? data.write(to: URL(fileURLWithPath: debugDirectory).appendingPathComponent("\(copy.name).png"))
    }

    let geometry = foregroundGeometry(image)
    precondition(!geometry.text.isEmpty, "\(copy.name): title/description produced no visible ink")
    precondition(!geometry.allow.isEmpty, "\(copy.name): translated Allow button lost its blue control")
    precondition(geometry.textBands.count == copy.expectedTextBands,
                 "\(copy.name): expected \(copy.expectedTextBands) visible text lines including title, got \(geometry.textBands)")
    precondition(geometry.text.minX > Int(80 * 2), "\(copy.name): text ink escaped its leading card inset")
    precondition(geometry.text.maxX < geometry.allow.minX - 8,
                 "\(copy.name): text ink reached the Allow control (text max x=\(geometry.text.maxX), Allow min x=\(geometry.allow.minX))")
    precondition(geometry.text.minY > 0 && geometry.text.maxY < rep.pixelsHigh - 1,
                 "\(copy.name): text touches the snapshot edge and may be vertically clipped")
    precondition(NSApp.windows.allSatisfy { !$0.isVisible }, "\(copy.name): text-layout smoke opened a visible window")

    let bands = geometry.textBands.map { "\($0.lowerBound)...\($0.upperBound)" }.joined(separator: ",")
    print("\(copy.name): card=518x80pt textBands=\(bands) textX=\(geometry.text.minX)...\(geometry.text.maxX) allowX=\(geometry.allow.minX)...\(geometry.allow.maxX)")
}

@main
enum PermissionCardTextLayoutSmoke {
    @MainActor
    static func main() {
        _ = NSApplication.shared

        // Exact accessibility-row copy from the shipped regional catalog.
        guard CommandLine.arguments.count > 1,
              let data = CommandLine.arguments[1].data(using: .utf8),
              let copies = try? JSONSerialization.jsonObject(with: data) as? [[String: String]],
              copies.count == 2 else {
            fatalError("Bun test must provide exact bg-BG and el-GR catalog entries as JSON")
        }
        assertCard(CardCopy(name: "bg-BG", values: copies[0], expectedTextBands: 3))
        assertCard(CardCopy(name: "el-GR", values: copies[1], expectedTextBands: 3))

        // A single unbroken token forces SwiftUI's emergency line breaking.
        // This is a stress probe, not a claim that arbitrary text must stay 80pt tall.
        var pressureCopy = copies[0]
        pressureCopy["permissionTitle"] = "Accessibility"
        pressureCopy["permissionDescription"] = String(repeating: "W", count: 72)
        pressureCopy["repair"] = "Allow"
        pressureCopy["title"] = "Enable ChatGPT scripting"
        pressureCopy["body"] = "Allow Accessibility access."
        assertCard(CardCopy(name: "unbroken-word", values: pressureCopy, expectedTextBands: 4))
        precondition(NSApp.windows.allSatisfy { !$0.isVisible }, "text-layout smoke opened a visible window")
        print("permission card text layout smoke passed (windowless; 2× SwiftUI foreground snapshots)")
    }
}
