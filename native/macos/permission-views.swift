import AppKit
import SwiftUI

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
        ZStack {
            shape.fill(.regularMaterial)
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
            .clipShape(shape)
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

private func permissionCopyString(_ copy: NSDictionary, _ key: String) -> String {
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

@MainActor
private final class PermissionInitialState: ObservableObject {
    @Published var title = ""
    @Published var body = ""
    @Published var permissionTitle = ""
    @Published var permissionDescription = ""
    @Published var allow = ""
    @Published var skip = ""
    @Published var completeInSettings = ""
    @Published var appIcon: NSImage?
    @Published var permissionIcon: NSImage?
    @Published var allowEnabled = true
    @Published var settingsPlaceholder = false
    @Published var placeholderHovered = false
    @Published var layoutDirection: LayoutDirection = .leftToRight
    weak var actionTarget: NSObject?

    func configure(
        copy: NSDictionary,
        appIcon: NSImage?,
        permissionIcon: NSImage?,
        actionTarget: NSObject?,
    ) {
        title = permissionCopyString(copy, "title")
        body = permissionCopyString(copy, "body")
        permissionTitle = permissionCopyString(copy, "permissionTitle")
        permissionDescription = permissionCopyString(copy, "permissionDescription")
        allow = permissionCopyString(copy, "repair")
        skip = permissionCopyString(copy, "later")
        completeInSettings = permissionCopyString(copy, "completeInSettings")
        self.appIcon = appIcon
        self.permissionIcon = permissionIcon
        self.actionTarget = actionTarget
        layoutDirection = permissionCopyString(copy, "layoutDirection") == "rightToLeft" ? .rightToLeft : .leftToRight
        allowEnabled = true
        settingsPlaceholder = false
        placeholderHovered = false
    }

    func setContent(
        title: String,
        body: String,
        allowEnabled: Bool,
        settingsPlaceholder: Bool,
    ) {
        self.title = title
        self.body = body
        self.allowEnabled = allowEnabled
        self.settingsPlaceholder = settingsPlaceholder
        // State transitions must not reuse a previous placeholder's hover.
        // A later genuine pointer entry may establish fresh hover normally.
        placeholderHovered = false
    }

    func send(_ selector: String) {
        precondition(Thread.isMainThread, "Permission actions must run on the main thread")
        guard let actionTarget else { return }
        _ = NSApp.sendAction(Selector(selector), to: actionTarget, from: nil)
    }
}

private struct PermissionEmbeddedView: NSViewRepresentable {
    let view: NSView
    let size: CGSize

    func makeNSView(context: Context) -> NSView { view }

    func updateNSView(_ nsView: NSView, context: Context) {
        // Do not touch isHidden here: Runtime deliberately hides the stable host
        // while taking permission-card snapshots and restores it independently.
        nsView.setFrameSize(size)
    }
}

private struct PermissionCardRoot: View {
    @ObservedObject var state: PermissionInitialState
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        // Original PermissionRow: zero stack spacing, independently padded
        // 64pt icon, then a flexible leading text column and trailing control.
        HStack(spacing: 0) {
            Group {
                if let image = state.permissionIcon {
                    Image(nsImage: image)
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                        .frame(width: 64, height: 64)
                } else {
                    Color.clear.frame(width: 64, height: 64)
                }
            }
            .padding(.leading, 8)
            .padding(.trailing, 12.5)

            VStack(alignment: .leading, spacing: 2) {
                Text(state.permissionTitle)
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(.primary)
                    // Preserve the measured CUA glyph origin (84.5, 20.5).
                    // Current SwiftUI places this glyph 1pt lower despite the
                    // same 16pt font and 2pt stack spacing; do not move the body.
                    .offset(y: -1)
                Text(state.permissionDescription)
                    .font(.system(size: 13))
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            Button(state.allow) { state.send("allow:") }
                .buttonStyle(.automatic)
                .keyboardShortcut(.defaultAction)
                .font(.system(size: 13))
                .clipShape(Capsule(style: .continuous))
                .frame(minWidth: 62)
                .offset(x: state.layoutDirection == .rightToLeft ? -4 : 4)
                .disabled(!state.allowEnabled)
        }
        .padding(.trailing, 20)
        .frame(width: 518, height: 80)
        .background {
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .fill(.regularMaterial)
        }
        .clipShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
        .overlay {
            if colorScheme == .dark {
                RoundedRectangle(cornerRadius: 24, style: .continuous)
                    .stroke(Color(nsColor: .separatorColor).opacity(0.75), lineWidth: 1)
            }
        }
        .shadow(color: .black.opacity(0.09), radius: 25, x: 0, y: 5)
        .shadow(color: .black.opacity(0.2), radius: 3, x: 0, y: 0)
        .environment(\.layoutDirection, state.layoutDirection)
    }
}

private struct PermissionPlaceholderLabel<Label: View>: View {
    let label: Label
    let pressed: Bool
    @ObservedObject var state: PermissionInitialState

    var body: some View {
        label
            .frame(maxWidth: .infinity, minHeight: 80)
            .background {
                RoundedRectangle(cornerRadius: 24, style: .continuous)
                    .fill(Color.primary.opacity(state.placeholderHovered ? 0.018 : 0))
            }
            .overlay {
                RoundedRectangle(cornerRadius: 24, style: .continuous)
                    .stroke(
                        Color.primary.opacity(pressed ? 0.22 : state.placeholderHovered ? 0.18 : 0.16),
                        style: StrokeStyle(lineWidth: 1, dash: [3, 6]),
                    )
            }
            .contentShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
            .onHover { state.placeholderHovered = $0 }
    }
}

private func permissionPlaceholderText(_ value: String) -> Text {
    let attributed = NSAttributedString(
        string: value,
        attributes: [
            .font: NSFont.systemFont(ofSize: 12, weight: .semibold),
            .kern: 0.7,
        ],
    )
    return Text(AttributedString(attributed))
}

private struct PermissionPlaceholderButtonStyle: ButtonStyle {
    let state: PermissionInitialState

    func makeBody(configuration: Configuration) -> some View {
        PermissionPlaceholderLabel(
            label: configuration.label,
            pressed: configuration.isPressed,
            state: state,
        )
    }
}

private struct PermissionInitialRoot: View {
    @ObservedObject var state: PermissionInitialState
    let cardHost: NSView

    var body: some View {
        ZStack(alignment: .topLeading) {
            VStack(spacing: 0) {
                if let image = state.appIcon {
                    Image(nsImage: image)
                        .interpolation(.high)
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                        .frame(width: 64, height: 64)
                        .padding(.top, 28)
                } else {
                    Color.clear.frame(width: 64, height: 64).padding(.top, 28)
                }

                Text(state.title)
                    .font(.system(size: 26, weight: .bold))
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(width: 560)
                    .padding(.top, 20)
                    .offset(y: -11)

                Text(state.body)
                    .font(.system(size: 13))
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .lineSpacing(2)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(width: 518)
                    .padding(.top, 3)

                ZStack {
                    PermissionEmbeddedView(view: cardHost, size: CGSize(width: 518, height: 80))
                    if state.settingsPlaceholder {
                        Button { state.send("resumeSettings:") } label: {
                            permissionPlaceholderText(state.completeInSettings)
                        }
                        .buttonStyle(PermissionPlaceholderButtonStyle(state: state))
                    }
                }
                .frame(width: 518, height: 80)
                .padding(.top, 21)
            }
            .frame(width: 600)
            .offset(y: -9)
            .padding(.bottom, 32)

            VStack {
                Spacer()
                HStack {
                    Spacer()
                    if !state.settingsPlaceholder {
                        Button(state.skip) { state.send("skip:") }
                            .buttonStyle(.plain)
                            .padding(.trailing, 57)
                    }
                }
                .padding(.bottom, 12.5)
            }
        }
        .frame(minWidth: 600, idealWidth: 600, maxWidth: 600, minHeight: 312, alignment: .top)
        .background(.regularMaterial)
        .environment(\.layoutDirection, state.layoutDirection)
    }
}

@MainActor
@objc(IncodexPermissionInitialView)
public final class IncodexPermissionInitialView: NSView {
    private let state: PermissionInitialState
    private let host: NSHostingView<PermissionInitialRoot>
    private let cardHost: NSHostingView<PermissionCardRoot>

    @objc public var permissionCardView: NSView { cardHost }

    @objc public var preferredContentSize: NSSize {
        precondition(Thread.isMainThread, "Permission layout must run on the main thread")
        let measured = host.fittingSize
        return NSSize(width: 600, height: max(312, measured.height))
    }

    public override var isFlipped: Bool { true }

    public override init(frame frameRect: NSRect) {
        precondition(Thread.isMainThread, "Permission views must be created on the main thread")
        let state = PermissionInitialState()
        let cardHost = NSHostingView(rootView: PermissionCardRoot(state: state))
        self.state = state
        self.cardHost = cardHost
        host = NSHostingView(rootView: PermissionInitialRoot(state: state, cardHost: cardHost))
        super.init(frame: frameRect)
        cardHost.frame = NSRect(x: 0, y: 0, width: 518, height: 80)
        cardHost.autoresizingMask = [.width, .height]
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

    @objc(configureWithCopy:appIcon:permissionIcon:actionTarget:)
    public func configure(
        copy: NSDictionary,
        appIcon: NSImage?,
        permissionIcon: NSImage?,
        actionTarget: NSObject?,
    ) {
        precondition(Thread.isMainThread, "Permission configuration must run on the main thread")
        state.configure(copy: copy, appIcon: appIcon, permissionIcon: permissionIcon, actionTarget: actionTarget)
    }

    @objc(setContentWithTitle:body:allowEnabled:settingsPlaceholder:)
    public func setContent(
        title: String,
        body: String,
        allowEnabled: Bool,
        settingsPlaceholder: Bool,
    ) {
        precondition(Thread.isMainThread, "Permission content must run on the main thread")
        state.setContent(title: title, body: body, allowEnabled: allowEnabled, settingsPlaceholder: settingsPlaceholder)
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
        let font = NSFont.preferredFont(forTextStyle: .body, options: [:])
        let rect = (instruction as NSString).boundingRect(
            with: NSSize(width: 408, height: CGFloat.greatestFiniteMagnitude),
            options: [.usesLineFragmentOrigin, .usesFontLeading],
            attributes: [.font: font],
        )
        return max(16, ceil(rect.height))
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

private struct PermissionHelperForeground<Row: View>: View {
    @ObservedObject var state: PermissionHelperState
    let appRowContent: Row
    var showHintArrow: Bool = false

    var body: some View {
        ZStack(alignment: .topLeading) {
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
                .offset(x: state.positionX(66, width: 28), y: 8.5)

            Text(state.styledInstruction)
                .font(.body)
                .fixedSize(horizontal: false, vertical: true)
                .frame(width: 408, alignment: .leading)
                .environment(\.layoutDirection, state.layoutDirection)
                .offset(x: state.positionX(102, width: 408), y: 17)

            appRowContent
                .frame(width: 459, height: 42)
                .offset(x: state.positionX(62, width: 459), y: 48 + state.extraHeight)

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
            .offset(x: state.positionX(18, width: 28), y: 55 + state.extraHeight)
        }
        .frame(width: 531, height: 110 + state.extraHeight, alignment: .topLeading)
        // Physical positions above already incorporate direction. Keep this
        // coordinate container LTR to avoid applying a second implicit mirror.
        .environment(\.layoutDirection, .leftToRight)
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
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(Color(nsColor: .separatorColor), lineWidth: 0.5)
        }
    }
}

@MainActor
@objc(IncodexPermissionHelperView)
public final class IncodexPermissionHelperView: NSView {
    private let state: PermissionHelperState
    private let host: NSHostingView<PermissionHelperRoot>
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

        if #available(macOS 13.0, *) {
            let renderer = ImageRenderer(content: foreground)
            renderer.proposedSize = ProposedViewSize(size)
            renderer.scale = scale
            return renderer.nsImage
        }

        // macOS 12 has no ImageRenderer. Render the same native SwiftUI
        // foreground offscreen, never the live host or an approximate AppKit UI.
        let snapshotHost = NSHostingView(rootView: foreground)
        snapshotHost.appearance = effectiveAppearance
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
        host = NSHostingView(rootView: PermissionHelperRoot(state: state, appRowBox: appRowBox))
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
    }
}
