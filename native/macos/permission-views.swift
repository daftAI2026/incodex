/**
 * [INPUT]: 依赖 AppKit/SwiftUI、当前系统权限词表与 UTType 图标。
 * [OUTPUT]: 提供共用呈现适配、飞行/helper ABI 和前景快照。
 * [POS]: CLI 与兼容 Runtime 共用边界；不检测或请求 TCC 权限。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import AppKit
import QuartzCore
import SwiftUI
import UniformTypeIdentifiers

// ==================== 系统呈现 ====================
@objc(IncodexPermissionPlatform)
public final class IncodexPermissionPlatform: NSObject {
    private static let systemLabels: [String: [String: String]] = {
        let path = "/System/Library/ExtensionKit/Extensions/SecurityPrivacyExtension.appex/Contents/Resources/Localizable.loctable"
        guard let data = try? Data(contentsOf: URL(fileURLWithPath: path)),
              let table = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any]
        else { return [:] }
        return stringLabels(table)
    }()

    static func stringLabels(_ table: [String: Any]) -> [String: [String: String]] {
        table.compactMapValues { ($0 as? [String: Any])?.compactMapValues { $0 as? String } }
    }

    @objc(copyForCurrentSystem:)
    public class func copyForCurrentSystem(_ copy: NSDictionary) -> NSDictionary {
        let major = ProcessInfo.processInfo.operatingSystemVersion.majorVersion
        guard major >= 27 else { return copy }
        return resolve(copy, majorVersion: major, labels: systemLabels)
    }

    static func resolve(_ copy: NSDictionary, majorVersion: Int, labels: [String: [String: String]]) -> NSDictionary {
        guard majorVersion >= 27, let locale = copy["permissionLocale"] as? String, !locale.isEmpty else { return copy }
        let requested = Locale(identifier: locale).languageCode ?? ""
        let available = labels.keys.filter {
            let language = Locale(identifier: $0).languageCode ?? ""
            return labels[$0]?["ACCESSIBILITY"] != nil && (language == requested || language == "en" ||
                (requested == "nb" && language == "no"))
        }.sorted()
        let preferred = Bundle.preferredLocalizations(from: available, forPreferences: [locale, "en"])
        guard let language = preferred.first, let strings = labels[language] else { return copy }
        let result = copy.mutableCopy() as! NSMutableDictionary
        let oldTitle = copy["permissionTitle"] as? String ?? ""
        let title = strings["ACCESSIBILITY"]?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        if !title.isEmpty, !oldTitle.isEmpty, title != oldTitle,
           let instruction = copy["permissionListInstruction"] as? String,
           let rawRuns = copy["permissionListInstructionRuns"] as? String,
           let data = rawRuns.data(using: .utf8),
           let runs = try? JSONSerialization.jsonObject(with: data) as? [[String: String]] {
            let expanded = instruction.replacingOccurrences(of: "{permissionTitle}", with: title)
            let expandedRuns = runs.map { run in
                run.mapValues { $0.replacingOccurrences(of: "{permissionTitle}", with: title) }
            }
            if !expanded.isEmpty, expandedRuns.map({ $0["text"] ?? "" }).joined() == expanded,
               let encoded = try? JSONSerialization.data(withJSONObject: expandedRuns, options: [.sortedKeys]),
               let runString = String(data: encoded, encoding: .utf8) {
                result["permissionTitle"] = title
                result["addedBody"] = expanded
                result["dragInstruction"] = expanded
                result["dragInstructionRuns"] = runString
                if let error = copy["errorBody"] as? String {
                    result["errorBody"] = error.replacingOccurrences(of: oldTitle, with: title)
                }
            }
        }
        let oldScreen = copy["screenTitle"] as? String ?? ""
        let screen = strings["SCREEN_CAPTURE_HEADER"]?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        if !screen.isEmpty, !oldScreen.isEmpty, screen != oldScreen {
            result["screenTitle"] = screen
            for key in ["screenAddedBody", "screenDragInstruction", "screenUnknown", "screenChecking"] {
                if let value = copy[key] as? String { result[key] = value.replacingOccurrences(of: oldScreen, with: screen) }
            }
            if let raw = copy["screenDragInstructionRuns"] as? String, let data = raw.data(using: .utf8),
               let runs = try? JSONSerialization.jsonObject(with: data) as? [[String: String]],
               let encoded = try? JSONSerialization.data(withJSONObject: runs.map { run in
                   run.mapValues { $0.replacingOccurrences(of: oldScreen, with: screen) }
               }, options: [.sortedKeys]) { result["screenDragInstructionRuns"] = String(data: encoded, encoding: .utf8) }
        }
        return result.copy() as! NSDictionary
    }

    static func accessibilityIconType(majorVersion: Int) -> String? {
        majorVersion >= 27 ? "com.apple.settings.PrivacySecurity.extension.privacy-accessibility" : nil
    }

    @MainActor @objc(accessibilityIcon)
    public class func accessibilityIcon() -> NSImage? {
        if let identifier = accessibilityIconType(majorVersion: ProcessInfo.processInfo.operatingSystemVersion.majorVersion),
           let type = UTType(identifier), type.isDeclared { return NSWorkspace.shared.icon(for: type) }
        return NSImage(contentsOfFile: "/System/Library/ExtensionKit/Extensions/AccessibilitySettingsExtension.appex/Contents/Resources/UniversalAccessPref.icns")
            ?? NSImage(contentsOfFile: "/System/Library/PreferencePanes/UniversalAccessPref.prefPane/Contents/Resources/UniversalAccessPref.icns")
            ?? NSImage(systemSymbolName: "accessibility", accessibilityDescription: nil)
    }
}

// Both transition endpoints render their shared SwiftUI foreground, not a
// screenshot of the live material and shadow shell.
@MainActor
func permissionForegroundSnapshot<Content: View>(
    content: Content, size: NSSize, scale: CGFloat, appearance: NSAppearance,
    useHostingView: Bool = false
) -> NSImage? {
    guard scale.isFinite, scale > 0, scale <= 8 else { return nil }
    if #available(macOS 13.0, *), !useHostingView {
        let renderer = ImageRenderer(content: content)
        renderer.proposedSize = ProposedViewSize(size)
        renderer.scale = scale
        return renderer.nsImage
    }

    // The permission-card Button renders a forbidden-operation glyph when
    // ImageRenderer runs inside the interactive host. Use the same SwiftUI
    // foreground in an offscreen NSHostingView instead; this also covers 12.x.
    let snapshotHost = NSHostingView(rootView: content)
    snapshotHost.appearance = appearance
    snapshotHost.frame = NSRect(origin: .zero, size: size)
    snapshotHost.layoutSubtreeIfNeeded()
    guard let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: Int(ceil(size.width * scale)),
        pixelsHigh: Int(ceil(size.height * scale)), bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
    ) else { return nil }
    bitmap.size = size
    snapshotHost.cacheDisplay(in: snapshotHost.bounds, to: bitmap)
    let image = NSImage(size: size)
    image.addRepresentation(bitmap)
    return image
}

// AppKit owns the host lifetime; TypeScript owns the externally sampled motion.
// Keep the concrete SwiftUI root alive across display ticks.
@MainActor
private final class PermissionFlightState: ObservableObject {
    @Published var source: NSImage?
    @Published var target: NSImage?
    @Published var progress: Double = 0
    @Published var cornerRadius: Double = 24
    @Published var reduceTransparency = false
}

private struct PermissionFlightImage: View {
    let image: NSImage?
    let opacity: Double
    let blurRadius: Double

    var body: some View {
        if let image {
            Image(nsImage: image)
                .interpolation(.high)
                .frame(width: image.size.width, height: image.size.height)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
                .opacity(opacity)
                .blur(radius: blurRadius)
        }
    }
}

private struct PermissionFlightRoot: View {
    @ObservedObject var state: PermissionFlightState

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: state.cornerRadius, style: .continuous)
        // The helper image is larger than the card image. Its intrinsic size
        // must not grow the material and shared clip after Back shrinks the
        // AppKit surface to the card's current frame.
        GeometryReader { geometry in
            shape.fill(.regularMaterial)
                .frame(width: geometry.size.width, height: geometry.size.height)
                .overlay {
                    ZStack {
                        PermissionFlightImage(
                            image: state.source,
                            opacity: 1 - state.progress,
                            blurRadius: state.reduceTransparency ? 0 : 12 * state.progress
                        )
                        PermissionFlightImage(
                            image: state.target,
                            opacity: state.progress,
                            blurRadius: state.reduceTransparency ? 0 : 12 * (1 - state.progress)
                        )
                    }
                    .frame(width: geometry.size.width, height: geometry.size.height)
                    .clipShape(shape)
                }
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

// Only this non-generic ObjC surface crosses the existing objc-js bridge.
// Call on the AppKit main thread; Back already swaps images in TypeScript.
@MainActor
@objc(IncodexPermissionFlightView)
public final class IncodexPermissionFlightView: NSView {
    private let state: PermissionFlightState
    private let host: NSHostingView<PermissionFlightRoot>

    @objc public var flightProgress: Double { state.progress }
    @objc public var flightCornerRadius: Double { state.cornerRadius }

    public override init(frame frameRect: NSRect) {
        let state = PermissionFlightState()
        self.state = state
        host = NSHostingView(rootView: PermissionFlightRoot(state: state))
        super.init(frame: frameRect)
        wantsLayer = true
        layer?.masksToBounds = false
        host.frame = bounds
        host.autoresizingMask = [.width, .height]
        addSubview(host)
    }

    @available(*, unavailable)
    public required init?(coder: NSCoder) {
        fatalError("Use init(frame:)")
    }

    public override func layout() {
        super.layout()
        host.frame = bounds
    }

    public override func hitTest(_ point: NSPoint) -> NSView? { nil }

    @objc(setSourceImage:targetImage:)
    public func setSourceImage(_ source: NSImage?, targetImage: NSImage?) {
        withoutImplicitAnimation {
            state.source = source
            state.target = targetImage
        }
    }

    @objc(updateProgress:cornerRadius:reduceTransparency:)
    public func updateProgress(_ progress: Double, cornerRadius: Double, reduceTransparency: Bool) {
        guard progress.isFinite, cornerRadius.isFinite else { return }
        withoutImplicitAnimation {
            state.progress = min(1, max(0, progress))
            state.cornerRadius = max(0, cornerRadius)
            state.reduceTransparency = reduceTransparency
        }
    }

    private func withoutImplicitAnimation(_ update: () -> Void) {
        var transaction = Transaction(animation: nil)
        transaction.disablesAnimations = true
        withTransaction(transaction, update)
    }
}

// The reference animation engine is driven by AppKit's display link, not a
// wall-clock timer.  Keep this small bridge separate from the SwiftUI flight
// surface so TypeScript can retain the existing spring state and stop it
// deterministically when the owned panels close.
@MainActor
@objc(IncodexPermissionDisplayLink)
public final class IncodexPermissionDisplayLink: NSObject {
    private var link: AnyObject?
    private var handler: ((Double, Double, Double) -> Void)?

    @objc public private(set) var displayLinked = false

    @objc(startForWindow:handler:)
    public func start(for window: NSWindow, handler: ((Double, Double, Double) -> Void)?) {
        precondition(Thread.isMainThread, "Permission display link must run on the main thread")
        invalidate()
        guard #available(macOS 14.0, *), window.screen != nil else {
            displayLinked = false
            return
        }
        self.handler = handler
        let displayLink = window.displayLink(target: self, selector: #selector(displayLinkFired(_:)))
        displayLink.add(to: .current, forMode: .common)
        link = displayLink
        displayLinked = true
    }

    @objc(startForScreen:handler:)
    public func start(for screen: NSScreen, handler: ((Double, Double, Double) -> Void)?) {
        precondition(Thread.isMainThread, "Permission display link must run on the main thread")
        invalidate()
        guard #available(macOS 14.0, *) else {
            displayLinked = false
            return
        }
        self.handler = handler
        let displayLink = screen.displayLink(target: self, selector: #selector(displayLinkFired(_:)))
        displayLink.add(to: .current, forMode: .common)
        link = displayLink
        displayLinked = true
    }

    @objc public func invalidate() {
        if #available(macOS 14.0, *) {
            (link as? CADisplayLink)?.invalidate()
        }
        link = nil
        handler = nil
        displayLinked = false
    }

    @available(macOS 14.0, *)
    @objc private func displayLinkFired(_ displayLink: CADisplayLink) {
        handler?(displayLink.timestamp, displayLink.duration, displayLink.targetTimestamp)
    }
}

func permissionCopyString(_ copy: NSDictionary, _ key: String) -> String {
    if let value = copy.object(forKey: key) as? String { return value }
    if let value = copy.object(forKey: key) as? NSString { return value as String }
    return ""
}

private var permissionBackFill: Color {
    if #available(macOS 14.0, *) {
        return Color(nsColor: .tertiarySystemFill)
    }
    // The original semantic fill API is unavailable on macOS 12/13.
    // Preserve the previous SwiftUI fill there; those OS versions do not
    // claim pixel parity with the macOS 14+ reference.
    return Color.primary.opacity(0.08)
}

private struct PermissionInstructionRun: Decodable {
    enum Role: String, Decodable { case primary, secondary }
    let text: String
    let role: Role
}

// Localized copy supplies semantic runs in its own natural order. Never
// infer names or word boundaries from English substrings in the renderer.
func permissionStyledInstruction(_ text: String, runsJSON: String) -> AttributedString {
    guard runsJSON.utf8.count <= 65_536,
          let data = runsJSON.data(using: .utf8),
          let runs = try? JSONDecoder().decode([PermissionInstructionRun].self, from: data),
          !runs.isEmpty, runs.count <= 128,
          runs.map(\.text).joined() == text else {
        return AttributedString(text)
    }
    var result = AttributedString()
    for run in runs {
        var part = AttributedString(run.text)
        part.foregroundColor = run.role == .primary ? Color.primary : Color.secondary
        result.append(part)
    }
    return result
}

struct PermissionEmbeddedView: NSViewRepresentable {
    let view: NSView
    let size: CGSize

    func makeNSView(context: Context) -> NSView { view }

    func updateNSView(_ nsView: NSView, context: Context) {
        // Do not touch isHidden here: Runtime deliberately hides the stable host
        // while taking permission-card snapshots and restores it independently.
        nsView.setFrameSize(size)
    }
}

// Original DraggableApplicationView sizes its NSImage to 32pt before the
// ApplicationRowView uses Image(nsImage:). Copy to avoid resizing the shared
// initial-window icon through the bridge.
func permissionHelperAppIcon(_ image: NSImage?) -> NSImage? {
    guard let copy = image?.copy() as? NSImage else { return nil }
    copy.size = NSSize(width: 32, height: 32)
    return copy
}

@MainActor
private final class PermissionHelperState: ObservableObject {
    @Published var instruction = ""
    @Published var styledInstruction = AttributedString()
    @Published var layoutDirection: LayoutDirection = .leftToRight
    @Published var back = ""
    @Published var appIcon: NSImage?
    weak var actionTarget: NSObject?

    var instructionHeight: CGFloat {
        // Measure the same attributed SwiftUI Text shown in the natural hint
        // HStack. The old 408pt NSString estimate could wrap while the live
        // 429pt proposal stayed on one line, needlessly raising the helper.
        let availableWidth: CGFloat = 531 - 18 - 28 - 16 - 4 - 28 - 8
        let label = Text(styledInstruction)
            .font(.body)
            .frame(width: availableWidth, alignment: .leading)
            .environment(\.layoutDirection, layoutDirection)
        return max(16, ceil(NSHostingView(rootView: label).fittingSize.height))
    }

    var extraHeight: CGFloat { max(0, instructionHeight - 16) }

    // AppKit overlays use physical coordinates, whereas the hosted SwiftUI
    // content uses semantic leading/trailing. Mirror the fixed guide anchors
    // explicitly so the drag overlay and rendered/snapshot row stay together.
    func positionX(_ left: CGFloat, width: CGFloat) -> CGFloat {
        layoutDirection == .rightToLeft ? 531 - left - width : left
    }

    func configure(copy: NSDictionary, appIcon: NSImage?, actionTarget: NSObject?) {
        instruction = permissionCopyString(copy, "dragInstruction")
        if instruction.isEmpty { instruction = permissionCopyString(copy, "addedBody") }
        if instruction.isEmpty { instruction = permissionCopyString(copy, "body") }
        styledInstruction = permissionStyledInstruction(
            instruction, runsJSON: permissionCopyString(copy, "dragInstructionRuns")
        )
        back = permissionCopyString(copy, "back")
        layoutDirection = permissionCopyString(copy, "layoutDirection") == "rightToLeft" ? .rightToLeft : .leftToRight
        self.appIcon = permissionHelperAppIcon(appIcon)
        self.actionTarget = actionTarget
    }

    func send(_ selector: String) {
        precondition(Thread.isMainThread, "Permission actions must run on the main thread")
        guard let actionTarget else { return }
        _ = NSApp.sendAction(Selector(selector), to: actionTarget, from: nil)
    }
}

private struct PermissionHelperAppRowRoot: View {
    @ObservedObject var state: PermissionHelperState

    var body: some View {
        HStack(alignment: .center, spacing: 4) {
            if let image = state.appIcon {
                Image(nsImage: image)
            } else {
                Color.clear.frame(width: 32, height: 32)
            }
            Text("ChatGPT")
                .foregroundStyle(.primary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .environment(\.layoutDirection, state.layoutDirection)
    }
}

// The live draggable row is an AppKit NSBox containing the stable SwiftUI
// app row. Its default 1pt border and (4,5) margins produce a 449x30 content
// frame at (5,6); the intrinsic 32pt image extends 1pt above and below it.
@MainActor
private final class PermissionHelperRowBox: NSBox {
    override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        boxType = .custom
        titlePosition = .noTitle
        cornerRadius = 7
        contentViewMargins = NSSize(width: 4, height: 5)
        updateColors()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("Use init(frame:)") }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        updateColors()
    }

    private func updateColors() {
        let dark = effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
        fillColor = dark ? NSColor.black.withAlphaComponent(0.06) : NSColor.white.withAlphaComponent(0.65)
        borderColor = dark ? NSColor.textColor.withAlphaComponent(0.08)
            : NSColor(srgbRed: 223 / 255, green: 221 / 255, blue: 227 / 255, alpha: 1)
        needsDisplay = true
    }
}

// SnapshotDraggableApplicationView has its own SwiftUI shape shell, not an
// NSViewRepresentable. Keep it separate from the live NSBox; its exact shape
// constants are independently closed by the original snapshot body witness.
private struct PermissionHelperSnapshotRow: View {
    @Environment(\.colorScheme) private var colorScheme
    @ObservedObject var state: PermissionHelperState

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 7, style: .continuous)
        let fill = colorScheme == .dark ? Color.black.opacity(0.06) : Color.white.opacity(0.65)
        let border = colorScheme == .dark ? Color(nsColor: .textColor).opacity(0.08)
            : Color(nsColor: NSColor(srgbRed: 223 / 255, green: 221 / 255, blue: 227 / 255, alpha: 1))
        shape.fill(fill)
            .overlay { shape.stroke(border, lineWidth: 1) }
            .overlay {
                PermissionHelperAppRowRoot(state: state)
                    .padding(.horizontal, 4)
                    .padding(.vertical, 5)
            }
            .frame(height: 42)
    }
}

// The approved custom glyph also participates in flight snapshots. The live
// animated glyph remains in its separate AppKit panel, not this foreground.
private struct PermissionSnapshotArrow: Shape {
    func path(in rect: CGRect) -> Path {
        func point(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
            CGPoint(x: rect.minX + (2 + x * 24 / 256) * rect.width / 28,
                    y: rect.minY + (2 + y * 24 / 256) * rect.height / 28)
        }
        var path = Path()
        path.move(to: point(128, 20))
        path.addLine(to: point(232, 116))
        path.addCurve(to: point(232, 132), control1: point(238.25, 122.25), control2: point(238.25, 125.75))
        path.addLine(to: point(200, 164))
        path.addCurve(to: point(184, 164), control1: point(193.75, 170.25), control2: point(190.25, 170.25))
        path.addLine(to: point(160, 140))
        path.addLine(to: point(160, 224))
        path.addCurve(to: point(152, 232), control1: point(160, 228.42), control2: point(156.42, 232))
        path.addLine(to: point(104, 232))
        path.addCurve(to: point(96, 224), control1: point(99.58, 232), control2: point(96, 228.42))
        path.addLine(to: point(96, 140))
        path.addLine(to: point(72, 164))
        path.addCurve(to: point(56, 164), control1: point(65.75, 170.25), control2: point(62.25, 170.25))
        path.addLine(to: point(24, 132))
        path.addCurve(to: point(24, 116), control1: point(17.75, 125.75), control2: point(17.75, 122.25))
        path.closeSubpath()
        return path
    }
}

@MainActor
private final class PermissionArrowState: ObservableObject {
    @Published var scaleX: CGFloat = 1
    @Published var scaleY: CGFloat = 1
}

// Keep the live and snapshot glyph on the same SwiftUI Shape. AppKit still
// owns the separate child panel and tracking overlay, while SwiftUI owns the
// animated transform so an interrupted spring can continue from its current
// presentation state instead of resetting a CA transform velocity.
private struct PermissionLiveArrowRoot: View {
    @ObservedObject var state: PermissionArrowState

    var body: some View {
        PermissionSnapshotArrow()
            .fill(Color(.sRGB, red: 0, green: 107 / 255, blue: 1, opacity: 1))
            .overlay {
                PermissionSnapshotArrow()
                    .stroke(.white, style: StrokeStyle(lineWidth: 2, lineJoin: .round))
            }
            .frame(width: 28, height: 28)
            .scaleEffect(x: state.scaleX, y: state.scaleY, anchor: .bottom)
            .shadow(color: .black.opacity(0.23), radius: 7, x: 0, y: 4)
            .allowsHitTesting(false)
    }
}

@MainActor
@objc(IncodexPermissionArrowView)
public final class IncodexPermissionArrowView: NSView {
    private let state: PermissionArrowState
    private let host: NSHostingView<PermissionLiveArrowRoot>

    public override var isFlipped: Bool { true }

    public override init(frame frameRect: NSRect) {
        precondition(Thread.isMainThread, "Permission arrow must be created on the main thread")
        let state = PermissionArrowState()
        self.state = state
        host = NSHostingView(rootView: PermissionLiveArrowRoot(state: state))
        super.init(frame: frameRect)
        clipsToBounds = false
        host.frame = bounds
        host.autoresizingMask = [.width, .height]
        host.clipsToBounds = false
        host.wantsLayer = true
        host.layer?.masksToBounds = false
        addSubview(host)
    }

    @available(*, unavailable)
    public required init?(coder: NSCoder) { fatalError("Use init(frame:)") }

    public override func layout() {
        super.layout()
        host.frame = bounds
    }

    public override func hitTest(_ point: NSPoint) -> NSView? { nil }

    @objc(animateToScaleX:scaleY:)
    public func animate(toScaleX x: Double, scaleY y: Double) {
        guard x.isFinite, y.isFinite, x > 0, y > 0 else { return }
        withAnimation(.interpolatingSpring(mass: 1, stiffness: 200, damping: 11, initialVelocity: 0)) {
            state.scaleX = CGFloat(x)
            state.scaleY = CGFloat(y)
        }
    }

    @objc(resetToIdentity)
    public func resetToIdentity() {
        var transaction = Transaction(animation: nil)
        transaction.disablesAnimations = true
        withTransaction(transaction) {
            state.scaleX = 1
            state.scaleY = 1
        }
    }
}

private struct PermissionHelperDragHint: View {
    @ObservedObject var state: PermissionHelperState
    let showHintArrow: Bool

    var body: some View {
        HStack(alignment: .center, spacing: 8) {
            PermissionSnapshotArrow()
                .fill(Color(.sRGB, red: 0, green: 107 / 255, blue: 1, opacity: 1))
                .overlay {
                    PermissionSnapshotArrow().stroke(.white, style: StrokeStyle(lineWidth: 2, lineJoin: .round))
                }
                .frame(width: 28, height: 28)
                .frame(width: 28, height: 32.5)
                .shadow(color: .black.opacity(0.23), radius: 7, x: 0, y: 4)
                .opacity(showHintArrow ? 1 : 0)
                .accessibilityHidden(true)
                .allowsHitTesting(false)

            Text(state.styledInstruction)
                .font(.body)
        }
    }
}

private struct PermissionHelperForeground<Row: View>: View {
    @ObservedObject var state: PermissionHelperState
    let appRowContent: Row
    var showHintArrow: Bool = false

    var body: some View {
        HStack(alignment: .bottom, spacing: 16) {
            Button { state.send("later:") } label: {
                Image(systemName: "chevron.left")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(.primary)
                    .scaleEffect(x: state.layoutDirection == .rightToLeft ? -1 : 1, y: 1)
                    .frame(width: 28, height: 28)
                    .background(permissionBackFill, in: Circle())
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
            .help(state.back)
            .accessibilityLabel(state.back)
            .frame(width: 28, height: 28)
            .padding(.bottom, 27)

            VStack(alignment: .leading, spacing: 7) {
                PermissionHelperDragHint(state: state, showHintArrow: showHintArrow)
                    .padding(.leading, 4)

                appRowContent
                    .frame(height: 42)
                    .padding(.trailing, 10)
            }
            .padding(.bottom, 20)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
        }
        .padding(.leading, 18)
        // Preserve the verified native shell's 110pt height and the product's
        // long-copy expansion; the reference's inner content frame is 108pt.
        .frame(width: 531, height: 110 + state.extraHeight)
        .environment(\.layoutDirection, state.layoutDirection)
    }
}

private struct PermissionHelperRoot: View {
    @ObservedObject var state: PermissionHelperState
    let appRowBox: NSView

    var body: some View {
        PermissionHelperForeground(
            state: state,
            appRowContent: PermissionEmbeddedView(view: appRowBox, size: CGSize(width: 459, height: 42))
        )
        .background(.regularMaterial)
    }
}

// The utility titlebar is transparent and its full-size content is already
// laid out in window coordinates. Do not add a second titlebar inset when
// AppKit asks the embedded SwiftUI host for its fitting size.
private final class PermissionHelperHostingView: NSHostingView<PermissionHelperRoot> {
    override var safeAreaInsets: NSEdgeInsets { NSEdgeInsets() }
}

@MainActor
@objc(IncodexPermissionHelperView)
public final class IncodexPermissionHelperView: NSView {
    private let state: PermissionHelperState
    private let host: PermissionHelperHostingView
    private let appRowHost: NSHostingView<PermissionHelperAppRowRoot>
    private let appRowBox: PermissionHelperRowBox

    @objc public var appRowView: NSView { appRowHost }

    @objc public var appRowFrame: NSRect {
        NSRect(x: state.positionX(62, width: 459), y: 48 + state.extraHeight, width: 459, height: 42)
    }

    @objc public var preferredContentSize: NSSize {
        precondition(Thread.isMainThread, "Permission layout must run on the main thread")
        return NSSize(width: 531, height: max(110, host.fittingSize.height))
    }

    /// The original helper renders AccessorySnapshotForegroundView rather than
    /// capturing its live Material-backed hosting view. Keep the draggable live
    /// row attached to its host; ImageRenderer needs a pure SwiftUI snapshot row.
    @objc(snapshotImageWithScale:)
    public func snapshotImage(scale: CGFloat) -> NSImage? {
        precondition(Thread.isMainThread, "Permission snapshots must run on the main thread")
        guard scale.isFinite, scale > 0, scale <= 8 else { return nil }
        let size = preferredContentSize
        let colorScheme: ColorScheme = effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
            ? .dark : .light
        let foreground = PermissionHelperForeground(
            state: state, appRowContent: PermissionHelperSnapshotRow(state: state), showHintArrow: true
        ).environment(\.colorScheme, colorScheme)

        return permissionForegroundSnapshot(
            content: foreground, size: size, scale: scale, appearance: effectiveAppearance
        )
    }

    public override var isFlipped: Bool { true }

    public override init(frame frameRect: NSRect) {
        precondition(Thread.isMainThread, "Permission views must be created on the main thread")
        let state = PermissionHelperState()
        let appRowHost = NSHostingView(rootView: PermissionHelperAppRowRoot(state: state))
        let appRowBox = PermissionHelperRowBox(frame: NSRect(x: 0, y: 0, width: 459, height: 42))
        appRowHost.frame = NSRect(x: 0, y: 0, width: 449, height: 30)
        appRowHost.autoresizingMask = [.width, .height]
        appRowBox.contentView = appRowHost
        self.state = state
        self.appRowHost = appRowHost
        self.appRowBox = appRowBox
        host = PermissionHelperHostingView(rootView: PermissionHelperRoot(state: state, appRowBox: appRowBox))
        super.init(frame: frameRect)
        host.frame = bounds
        host.autoresizingMask = [.width, .height]
        addSubview(host)
    }

    @available(*, unavailable)
    public required init?(coder: NSCoder) { fatalError("Use init(frame:)") }

    public override func layout() {
        super.layout()
        host.frame = bounds
    }

    @objc(configureWithCopy:appIcon:actionTarget:)
    public func configure(copy: NSDictionary, appIcon: NSImage?, actionTarget: NSObject?) {
        precondition(Thread.isMainThread, "Permission configuration must run on the main thread")
        state.configure(copy: copy, appIcon: appIcon, actionTarget: actionTarget)
        // Configuration and measurement are one synchronous bridge handoff.
        // Flush the published state without replacing the embedded row host.
        host.rootView = PermissionHelperRoot(state: state, appRowBox: appRowBox)
        host.needsLayout = true
        host.layoutSubtreeIfNeeded()
    }
}
