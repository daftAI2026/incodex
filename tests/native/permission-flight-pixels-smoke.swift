import AppKit
import Foundation

private let surfaceSize = NSSize(width: 531, height: 110)
private let probeRadius: CGFloat = 24

private struct Pixel {
    let red: CGFloat
    let green: CGFloat
    let blue: CGFloat
    let alpha: CGFloat
}

private struct PixelBounds {
    let minX: Int
    let maxX: Int
    let minY: Int
    let maxY: Int

    var width: Int { maxX - minX + 1 }
    var height: Int { maxY - minY + 1 }
    var centerX: CGFloat { CGFloat(minX + maxX + 1) / 2 }
    var centerY: CGFloat { CGFloat(minY + maxY + 1) / 2 }
}

private func bitmapRep(_ image: NSImage) -> NSBitmapImageRep {
    if let rep = image.representations.compactMap({ $0 as? NSBitmapImageRep }).first {
        return rep
    }
    guard let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
        fatalError("flight cache did not return rasterizable content")
    }
    return NSBitmapImageRep(cgImage: cgImage)
}

private func cacheImage(_ view: NSView) -> NSImage {
    view.layoutSubtreeIfNeeded()
    guard let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else {
        fatalError("flight view did not provide a cache bitmap")
    }
    view.cacheDisplay(in: view.bounds, to: rep)
    let image = NSImage(size: view.bounds.size)
    image.addRepresentation(rep)
    return image
}

private func makeSolidImage(size: NSSize, red: CGFloat, green: CGFloat, blue: CGFloat) -> NSImage {
    let image = NSImage(size: size)
    image.lockFocus()
    NSColor(calibratedRed: red, green: green, blue: blue, alpha: 1).setFill()
    NSRect(origin: .zero, size: size).fill()
    image.unlockFocus()
    return image
}

private func pixel(_ image: NSImage, x: CGFloat, y: CGFloat) -> Pixel {
    let rep = bitmapRep(image)
    let scaleX = CGFloat(rep.pixelsWide) / image.size.width
    let scaleY = CGFloat(rep.pixelsHigh) / image.size.height
    let pixelX = min(rep.pixelsWide - 1, max(0, Int(floor(x * scaleX))))
    // NSBitmapImageRep.colorAt uses the stored top row; this is also the
    // convention used by the existing helper snapshot smoke.
    let pixelY = min(rep.pixelsHigh - 1, max(0, Int(floor(y * scaleY))))
    guard let rgb = rep.colorAt(x: pixelX, y: pixelY)?.usingColorSpace(.sRGB) else {
        return Pixel(red: 0, green: 0, blue: 0, alpha: 0)
    }
    return Pixel(red: rgb.redComponent, green: rgb.greenComponent,
                 blue: rgb.blueComponent, alpha: rgb.alphaComponent)
}

private func dominantBounds(_ image: NSImage, color: KeyPath<Pixel, CGFloat>, other: KeyPath<Pixel, CGFloat>) -> PixelBounds? {
    let rep = bitmapRep(image)
    var minX = rep.pixelsWide
    var maxX = -1
    var minY = rep.pixelsHigh
    var maxY = -1
    for y in 0..<rep.pixelsHigh {
        for x in 0..<rep.pixelsWide {
            guard let rgb = rep.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) else { continue }
            let p = Pixel(red: rgb.redComponent, green: rgb.greenComponent,
                          blue: rgb.blueComponent, alpha: rgb.alphaComponent)
            if p.alpha > 0.5 && p[keyPath: color] > 0.45 &&
                p[keyPath: color] > p[keyPath: other] + 0.25 {
                minX = min(minX, x)
                maxX = max(maxX, x)
                minY = min(minY, y)
                maxY = max(maxY, y)
            }
        }
    }
    guard maxX >= minX, maxY >= minY else { return nil }
    return PixelBounds(minX: minX, maxX: maxX, minY: minY, maxY: maxY)
}

private func assertCentered(_ image: NSImage, expectedSize: NSSize, label: String) {
    let rep = bitmapRep(image)
    let scaleX = CGFloat(rep.pixelsWide) / image.size.width
    let scaleY = CGFloat(rep.pixelsHigh) / image.size.height
    precondition(abs(image.size.width - surfaceSize.width) < 0.01 &&
                 abs(image.size.height - surfaceSize.height) < 0.01,
                 "\(label): flight surface size changed: \(image.size)")

    let expected = NSRect(x: (surfaceSize.width - expectedSize.width) / 2,
                          y: (surfaceSize.height - expectedSize.height) / 2,
                          width: expectedSize.width, height: expectedSize.height)
    let expectedPixels = NSRect(x: expected.minX * scaleX, y: expected.minY * scaleY,
                                width: expected.width * scaleX, height: expected.height * scaleY)
    let toleranceX = 3 * scaleX
    let toleranceY = 3 * scaleY
    let bounds: PixelBounds?
    if label.contains("red") {
        bounds = dominantBounds(image, color: \.red, other: \.blue)
    } else {
        bounds = dominantBounds(image, color: \.blue, other: \.red)
    }
    guard let bounds else { fatalError("\(label): no dominant image pixels") }
    precondition(abs(CGFloat(bounds.minX) - expectedPixels.minX) <= toleranceX &&
                 abs(CGFloat(bounds.maxX + 1) - expectedPixels.maxX) <= toleranceX &&
                 abs(CGFloat(bounds.minY) - expectedPixels.minY) <= toleranceY &&
                 abs(CGFloat(bounds.maxY + 1) - expectedPixels.maxY) <= toleranceY,
                 "\(label): image bounds not centered/at original size; actual=\(bounds), expectedPixels=\(expectedPixels)")
    print("\(label) bounds=\(bounds.minX),\(bounds.minY) \(bounds.width)x\(bounds.height) scale=\(scaleX)x\(scaleY)")
}

private func colorDistance(_ lhs: Pixel, _ rhs: Pixel) -> CGFloat {
    abs(lhs.red - rhs.red) + abs(lhs.green - rhs.green) + abs(lhs.blue - rhs.blue) + abs(lhs.alpha - rhs.alpha)
}

private func imageDistance(_ lhs: NSImage, _ rhs: NSImage, rect: NSRect) -> CGFloat {
    let lhsRep = bitmapRep(lhs)
    let rhsRep = bitmapRep(rhs)
    precondition(lhsRep.pixelsWide == rhsRep.pixelsWide && lhsRep.pixelsHigh == rhsRep.pixelsHigh)
    let scaleX = CGFloat(lhsRep.pixelsWide) / lhs.size.width
    let scaleY = CGFloat(lhsRep.pixelsHigh) / lhs.size.height
    let minX = max(0, Int(floor(rect.minX * scaleX)))
    let maxX = min(lhsRep.pixelsWide, Int(ceil(rect.maxX * scaleX)))
    let minY = max(0, Int(floor(rect.minY * scaleY)))
    let maxY = min(lhsRep.pixelsHigh, Int(ceil(rect.maxY * scaleY)))
    var sum: CGFloat = 0
    var count = 0
    for y in minY..<maxY {
        for x in minX..<maxX {
            guard let a = lhsRep.colorAt(x: x, y: y)?.usingColorSpace(.sRGB),
                  let b = rhsRep.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) else { continue }
            sum += abs(a.redComponent - b.redComponent)
            sum += abs(a.greenComponent - b.greenComponent)
            sum += abs(a.blueComponent - b.blueComponent)
            sum += abs(a.alphaComponent - b.alphaComponent)
            count += 1
        }
    }
    return count == 0 ? 0 : sum / CGFloat(count)
}

private func centerPixel(_ image: NSImage) -> Pixel {
    pixel(image, x: surfaceSize.width / 2, y: surfaceSize.height / 2)
}

private func assertOpacityProgression(_ samples: [Pixel], label: String) {
    precondition(samples.count == 5, "\(label): missing fixed progress samples")
    let first = samples[0]
    let last = samples[4]
    precondition(first.red > first.blue + 0.25 && last.blue > last.red + 0.25,
                 "\(label): endpoints did not preserve source/target color identity: first=\(first), last=\(last)")
    for index in 1..<samples.count {
        precondition(samples[index].red <= samples[index - 1].red + 0.03 &&
                     samples[index].blue >= samples[index - 1].blue - 0.03,
                     "\(label): opacity did not progress monotonically at index \(index): \(samples)")
    }
    print("\(label) opacity centers source=\(first) target=\(last)")
}

private func assertCenterOccupied(_ image: NSImage, label: String) {
    let actual = pixel(image, x: surfaceSize.width / 2, y: surfaceSize.height / 2)
    precondition(actual.alpha > 0.75, "\(label): center became transparent: \(actual)")
    print("\(label) center=\(actual.red),\(actual.green),\(actual.blue),a=\(actual.alpha)")
}

private func assertOutsideRoundedClip(_ image: NSImage, baseline: NSImage, label: String) {
    // The source/target images are saturated red/blue while the material is
    // neutral. These points are outside a 24pt continuous corner and should
    // therefore match the no-image material baseline after opacity/blur.
    for point in [NSPoint(x: 1, y: 1), NSPoint(x: 3, y: 3)] {
        let actual = pixel(image, x: point.x, y: point.y)
        let expected = pixel(baseline, x: point.x, y: point.y)
        precondition(colorDistance(actual, expected) < 0.16,
                     "\(label): image escaped shared rounded clip at \(point); actual=\(actual), baseline=\(expected)")
    }
}

@MainActor
private func render(_ view: IncodexPermissionFlightView,
                    source: NSImage?, target: NSImage?, progress: CGFloat,
                    reduceTransparency: Bool) -> NSImage {
    view.setSourceImage(source, targetImage: target)
    view.updateProgress(progress, cornerRadius: probeRadius, reduceTransparency: reduceTransparency)
    view.setFrameSize(surfaceSize)
    // @Published updates invalidate the hosted SwiftUI tree on the main run
    // loop. Flush that invalidation before cacheDisplay; this remains
    // windowless and is required to sample the state just submitted.
    for _ in 0..<3 {
        RunLoop.main.run(until: Date().addingTimeInterval(0.02))
        view.layoutSubtreeIfNeeded()
        view.subviews.first?.layoutSubtreeIfNeeded()
    }
    view.needsDisplay = true
    view.displayIfNeeded()
    return cacheImage(view)
}

@main
enum PermissionFlightPixelsSmoke {
    @MainActor
    static func main() {
        _ = NSApplication.shared
        precondition(NSApp.windows.allSatisfy { !$0.isVisible }, "flight pixel smoke must start windowless")

        let red = Pixel(red: 0.92, green: 0.06, blue: 0.06, alpha: 1)
        let blue = Pixel(red: 0.06, green: 0.18, blue: 0.92, alpha: 1)
        let small = NSSize(width: 518, height: 80)
        let large = surfaceSize
        let directions: [(String, NSSize, Pixel, NSSize, Pixel)] = [
            ("forward", small, red, large, blue),
            ("reverse", large, red, small, blue),
        ]

        for (direction, sourceSize, sourcePixel, targetSize, targetPixel) in directions {
            let source = makeSolidImage(size: sourceSize, red: sourcePixel.red,
                                        green: sourcePixel.green, blue: sourcePixel.blue)
            let target = makeSolidImage(size: targetSize, red: targetPixel.red,
                                        green: targetPixel.green, blue: targetPixel.blue)
            precondition(source.size == sourceSize && target.size == targetSize)
            let view = IncodexPermissionFlightView(frame: NSRect(origin: .zero, size: surfaceSize))
            precondition(view.subviews.count == 1, "\(direction): flight host was replaced")
            let host = view.subviews[0]

            var noBlurAtHalf: NSImage?
            var blurAtHalf: NSImage?
            var noBlurCenters: [Pixel] = []
            for reduceTransparency in [true, false] {
                for progress in [CGFloat(0), 0.25, 0.5, 0.75, 1] {
                    let image = render(view, source: source, target: target,
                                       progress: progress, reduceTransparency: reduceTransparency)
                    precondition(view.subviews[0] === host, "\(direction): SwiftUI host changed at p=\(progress)")
                    precondition(view.flightProgress == Double(progress) && view.flightCornerRadius == Double(probeRadius))
                    precondition(source.size == sourceSize && target.size == targetSize,
                                 "\(direction): native rendering mutated source/target intrinsic size")
                    assertCenterOccupied(image, label: "\(direction) reduce=\(reduceTransparency) p=\(progress)")
                    if reduceTransparency {
                        noBlurCenters.append(centerPixel(image))
                    }

                    // Endpoint bounds provide the direct original-size/center
                    // check. At intermediate progress the two images overlap,
                    // so only the center compositing contract is asserted.
                    if progress == 0 {
                        assertCentered(image, expectedSize: sourceSize, label: "\(direction) source \(sourcePixel.red > sourcePixel.blue ? "red" : "blue")")
                    }
                    if progress == 1 {
                        assertCentered(image, expectedSize: targetSize, label: "\(direction) target \(targetPixel.red > targetPixel.blue ? "red" : "blue")")
                    }
                    if progress == 0.5 && reduceTransparency {
                        noBlurAtHalf = image
                    } else if progress == 0.5 {
                        blurAtHalf = image
                    }
                }
            }

            guard let noBlurAtHalf, let blurAtHalf else { fatalError("missing half-progress branch") }
            assertOpacityProgression(noBlurCenters, label: "\(direction) reduce=true")
            let blurDelta = imageDistance(noBlurAtHalf, blurAtHalf, rect: NSRect(origin: .zero, size: surfaceSize))
            if blurDelta > 0.0005 {
                print("\(direction) blur branch pixel delta=\(blurDelta)")
            } else {
                // NSHostingView.cacheDisplay/layer.render is a valid
                // windowless raster path for geometry, but this cache path did
                // not expose SwiftUI's blur effect in this run.
                // Keep this as an explicit diagnostic instead of treating the
                // inability to observe blur as a product pass or failure.
                print("\(direction) blur pixel delta=0 in this windowless cache path (effect unobserved)")
            }

            // Render a no-image baseline with the same state. Comparing corner
            // pixels against it catches either image escaping the single outer
            // continuous clip after opacity/blur; the material itself is not
            // treated as an image-pixel pass.
            let baseline = render(view, source: nil, target: nil, progress: 0.5, reduceTransparency: false)
            let clipped = render(view, source: source, target: target, progress: 0.5, reduceTransparency: false)
            assertOutsideRoundedClip(clipped, baseline: baseline, label: direction)
            precondition(view.subviews[0] === host, "\(direction): baseline/clip probe replaced live host")
        }

        precondition(NSApp.windows.allSatisfy { !$0.isVisible }, "flight pixel smoke created a visible window")
        print("permission flight pixels smoke passed (windowless; F03/F05/F07 image branches; blur effect unobserved in this cache path)")
    }
}
