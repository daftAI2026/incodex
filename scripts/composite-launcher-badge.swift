/**
 * [INPUT]: 依赖 ImageIO 的原图与 Apple ictool 成品解码、布局中的胶囊轮廓。
 * [OUTPUT]: 输出局部替换预览；提供合成与布局自测，不改产品图标。
 * [POS]: 开发期 Icon Composer 适配；原生导出已经合成，禁止再次 source-over。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import Foundation
import CoreGraphics
import ImageIO

struct Failure: Error, CustomStringConvertible { let description: String }
func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw Failure(description: message) }
}
struct Layout: Decodable {
    let canvas: Int
    let x: Double
    let y: Double
    let width: Double
    let height: Double
    let radius: Double
    func validate() throws {
        try require(canvas > 0 && canvas <= 4096, "invalid canvas")
        try require([x, y, width, height, radius].allSatisfy(\.isFinite), "nonfinite layout")
        try require(x >= 0 && y >= 0 && width > 0 && height > 0, "invalid capsule")
        try require(x + width <= Double(canvas) && y + height <= Double(canvas), "capsule outside canvas")
        try require(radius >= 0 && radius <= min(width, height) / 2, "invalid radius")
    }
    func mask() throws -> [UInt8] {
        try validate()
        var bytes = [UInt8](repeating: 0, count: canvas * canvas)
        try bytes.withUnsafeMutableBytes { memory in
            guard let c = CGContext(data: memory.baseAddress, width: canvas, height: canvas,
                bitsPerComponent: 8, bytesPerRow: canvas, space: CGColorSpaceCreateDeviceGray(),
                bitmapInfo: CGImageAlphaInfo.none.rawValue) else { throw Failure(description: "mask context failed") }
            // 布局沿图像左上角计数；位图绘制坐标需翻转。
            c.translateBy(x: 0, y: CGFloat(canvas))
            c.scaleBy(x: 1, y: -1)
            c.setFillColor(gray: 1, alpha: 1)
            c.addPath(CGPath(roundedRect: CGRect(x: x, y: y, width: width, height: height),
                             cornerWidth: radius, cornerHeight: radius, transform: nil))
            c.fillPath()
        }
        return bytes
    }
}

// 原生输入已有边缘抗锯齿；mask 只选替换范围，不二次乘边缘覆盖率。
func compose(source: [UInt8], native: [UInt8], mask: [UInt8]) throws -> [UInt8] {
    try require(source.count == native.count && source.count == mask.count * 4, "pixel dimensions differ")
    var output = source
    for i in mask.indices where mask[i] > 0 {
        let p = i * 4
        output.replaceSubrange(p..<p + 4, with: native[p..<p + 4])
    }
    return output
}

func sourceSpace(_ path: String) throws -> CGColorSpace {
    guard let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(src, 0, nil),
          let space = image.colorSpace, space.model == .rgb else { throw Failure(description: "source needs an RGB profile") }
    return space
}
func read(_ path: String, canvas: Int, space: CGColorSpace) throws -> [UInt8] {
    guard let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(src, 0, nil) else { throw Failure(description: "cannot decode image: \(path)") }
    try require(image.width == image.height, "input image must be square")
    var bytes = [UInt8](repeating: 0, count: canvas * canvas * 4)
    try bytes.withUnsafeMutableBytes { memory in
        guard let c = CGContext(data: memory.baseAddress, width: canvas, height: canvas,
            bitsPerComponent: 8, bytesPerRow: canvas * 4, space: space,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue)
            else { throw Failure(description: "image context failed") }
        c.interpolationQuality = .high
        c.draw(image, in: CGRect(x: 0, y: 0, width: canvas, height: canvas))
    }
    return bytes
}
func save(_ bytes: [UInt8], canvas: Int, path: String, space: CGColorSpace) throws {
    guard let provider = CGDataProvider(data: Data(bytes) as CFData),
          let image = CGImage(width: canvas, height: canvas, bitsPerComponent: 8, bitsPerPixel: 32,
            bytesPerRow: canvas * 4, space: space,
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue),
            provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent),
          let destination = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL,
            "public.png" as CFString, 1, nil) else { throw Failure(description: "cannot create output") }
    CGImageDestinationAddImage(destination, image, nil)
    try require(CGImageDestinationFinalize(destination), "cannot save output")
}

// 与原图共用 RGB profile，原生导出只转换到这个目标空间。
func render(originalPath: String, nativePath: String, layout: Layout, outputPath: String) throws {
        let mask = try layout.mask()
        let space = try sourceSpace(originalPath)
        let source = try read(originalPath, canvas: layout.canvas, space: space)
        let native = try read(nativePath, canvas: layout.canvas, space: space)
        let result = try compose(source: source, native: native, mask: mask)
        var copied = 0, changedOutside = 0, formerlySkipped = 0, mismatchInside = 0
        for i in mask.indices {
            if mask[i] > 0 {
                copied += 1
                if source[i * 4 + 3] != 255 { formerlySkipped += 1 }
                if result[i * 4..<i * 4 + 4] != native[i * 4..<i * 4 + 4] { mismatchInside += 1 }
            }
            else if result[i * 4..<i * 4 + 4] != source[i * 4..<i * 4 + 4] { changedOutside += 1 }
        }
        try require(changedOutside == 0 && mismatchInside == 0, "mask replacement mismatch")
        try save(result, canvas: layout.canvas, path: outputPath, space: space)
        print("maskPixels=\(copied), outsideMaskChanged=\(changedOutside), insideNativeMismatch=\(mismatchInside), nonopaqueSourcePixelsReplaced=\(formerlySkipped)")
}

func selfTest() throws {
    let source: [UInt8] = [100, 100, 100, 254, 100, 100, 100, 255,
                           80, 80, 80, 254, 0, 0, 0, 0]
    let native: [UInt8] = [21, 25, 37, 255, 21, 25, 37, 255,
                           21, 25, 37, 255, 21, 25, 37, 255]
    let result = try compose(source: source, native: native, mask: [255, 255, 0, 0])
    try require(result[0..<4] == native[0..<4], "regression: alpha254 leaves a hole inside badge")
    try require(result[4..<8] == native[4..<8], "opaque positive control not painted")
    try require(result[8..<16] == source[8..<16], "outside mask changed")
    let edge = try compose(source: source, native: native, mask: [1, 127, 0, 0])
    try require(edge[0..<8] == native[0..<8], "native AA multiplied twice")
    let zero = try compose(source: source, native: native, mask: [0, 0, 0, 0])
    try require(zero == source, "zero mask changed image")
    let mask = try Layout(canvas: 32, x: 4, y: 18, width: 24, height: 8, radius: 4).mask()
    try require(mask[22 * 32 + 16] == 255 && mask[10 * 32 + 16] == 0, "mask coordinates inverted")
    try require(mask[18 * 32 + 4] == 0, "rounded corner became rectangular")
    let folder = FileManager.default.temporaryDirectory.appendingPathComponent("incodex-badge-png-" + UUID().uuidString)
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false)
    defer { try? FileManager.default.removeItem(at: folder) }
    let p3 = CGColorSpace(name: CGColorSpace.displayP3)!
    let originalPath = folder.appendingPathComponent("source.png").path
    let nativePath = folder.appendingPathComponent("native.png").path
    let outputPath = folder.appendingPathComponent("output.png").path
    let pixels: [UInt8] = [120, 20, 15, 254, 80, 100, 160, 255, 40, 80, 10, 254, 0, 0, 0, 0]
    try save(pixels, canvas: 2, path: originalPath, space: p3)
    try save(native, canvas: 2, path: nativePath, space: p3)
    try render(originalPath: originalPath, nativePath: nativePath,
               layout: Layout(canvas: 2, x: 0, y: 0, width: 2, height: 1, radius: 0), outputPath: outputPath)
    try require(try sourceSpace(outputPath).name == sourceSpace(originalPath).name, "PNG lost source profile")
    let before = try read(originalPath, canvas: 2, space: p3)
    let after = try read(outputPath, canvas: 2, space: p3)
    let target = try read(nativePath, canvas: 2, space: p3)
    try require(after[0..<8] == target[0..<8] && after[8..<16] == before[8..<16], "PNG roundtrip changed replacement or original")
    print("PASS alpha254, alpha255, outside-mask, native-AA, zero-mask, mask-orientation, rounded-corner, P3-profile, PNG-roundtrip")
}

do {
    if CommandLine.arguments.dropFirst() == ["--self-test"] {
        try selfTest()
    } else {
        try require(CommandLine.arguments.count == 5, "usage: swift composite-launcher-badge.swift original.png native.png layout.json output.png")
        let a = CommandLine.arguments
        let layout = try JSONDecoder().decode(Layout.self, from: Data(contentsOf: URL(fileURLWithPath: a[3])))
        try render(originalPath: a[1], nativePath: a[2], layout: layout, outputPath: a[4])
    }
} catch { fputs("\(error)\n", stderr); exit(1) }
