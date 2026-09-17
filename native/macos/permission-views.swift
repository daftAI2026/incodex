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
        allowEnabled = true
        settingsPlaceholder = false
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
        HStack(spacing: 10.5) {
            if let image = state.permissionIcon {
                Image(nsImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(width: 64, height: 64)
            } else {
                Color.clear.frame(width: 64, height: 64)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text(state.permissionTitle)
                    .font(.system(size: 16, weight: .semibold))
                    .lineLimit(1)
                Text(state.permissionDescription)
                    .font(.system(size: 13))
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            Button(state.allow) { state.send("allow:") }
                .buttonStyle(.automatic)
                .keyboardShortcut(.defaultAction)
                .font(.system(size: 13))
                .clipShape(Capsule())
                .frame(minWidth: 62)
                .offset(x: 4)
                .disabled(!state.allowEnabled)
        }
        .padding(8)
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
                        .resizable()
                        .aspectRatio(contentMode: .fit)
                        .frame(width: 64, height: 64)
                        .padding(.top, 28)
                } else {
                    Color.clear.frame(width: 64, height: 64).padding(.top, 28)
                }

                Text(state.title)
                    .font(.system(size: 26, weight: .bold))
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

@MainActor
private final class PermissionHelperState: ObservableObject {
    @Published var instruction = ""
    @Published var back = ""
    @Published var appIcon: NSImage?
    weak var actionTarget: NSObject?

    var instructionHeight: CGFloat {
        let font = NSFont.systemFont(ofSize: 13)
        let rect = (instruction as NSString).boundingRect(
            with: NSSize(width: 408, height: CGFloat.greatestFiniteMagnitude),
            options: [.usesLineFragmentOrigin, .usesFontLeading],
            attributes: [.font: font],
        )
        return max(16, ceil(rect.height))
    }

    var extraHeight: CGFloat { max(0, instructionHeight - 16) }

    func configure(copy: NSDictionary, appIcon: NSImage?, actionTarget: NSObject?) {
        instruction = permissionCopyString(copy, "dragInstruction")
        if instruction.isEmpty { instruction = permissionCopyString(copy, "addedBody") }
        if instruction.isEmpty { instruction = permissionCopyString(copy, "body") }
        back = permissionCopyString(copy, "back")
        self.appIcon = appIcon
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
        ZStack(alignment: .topLeading) {
            if let image = state.appIcon {
                Image(nsImage: image)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(width: 32, height: 32)
                    .offset(x: 5, y: 5)
            } else {
                Color.clear.frame(width: 32, height: 32).offset(x: 5, y: 5)
            }
            Text("ChatGPT")
                .font(.system(size: 13))
                .offset(x: 41, y: 13)
        }
        .frame(width: 459, height: 42, alignment: .topLeading)
    }
}

private struct PermissionHelperRoot: View {
    @ObservedObject var state: PermissionHelperState
    let appRowHost: NSView

    var body: some View {
        ZStack(alignment: .topLeading) {
            Text(state.instruction)
                .font(.system(size: 13))
                .fixedSize(horizontal: false, vertical: true)
                .frame(width: 408, alignment: .leading)
                .offset(x: 102, y: 17)

            ZStack {
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .fill(Color(nsColor: .controlBackgroundColor))
                PermissionEmbeddedView(view: appRowHost, size: CGSize(width: 459, height: 42))
            }
                .frame(width: 459, height: 42)
                .overlay {
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .stroke(Color(nsColor: .separatorColor), lineWidth: 0.5)
                }
                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                .offset(x: 62, y: 48 + state.extraHeight)

            Button { state.send("later:") } label: {
                Image(systemName: "chevron.left")
                    .font(.system(size: 12, weight: .semibold))
                    .frame(width: 28, height: 28)
            }
            .buttonStyle(.plain)
            .help(state.back)
            .accessibilityLabel(state.back)
            .frame(width: 28, height: 28)
            .background(Color.primary.opacity(0.08), in: Circle())
            .offset(x: 18, y: 55 + state.extraHeight)
        }
        .frame(width: 531, height: 110 + state.extraHeight, alignment: .topLeading)
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

    @objc public var appRowView: NSView { appRowHost }

    @objc public var appRowFrame: NSRect {
        NSRect(x: 62, y: 48 + state.extraHeight, width: 459, height: 42)
    }

    @objc public var preferredContentSize: NSSize {
        precondition(Thread.isMainThread, "Permission layout must run on the main thread")
        return NSSize(width: 531, height: max(110, host.fittingSize.height))
    }

    public override var isFlipped: Bool { true }

    public override init(frame frameRect: NSRect) {
        precondition(Thread.isMainThread, "Permission views must be created on the main thread")
        let state = PermissionHelperState()
        let appRowHost = NSHostingView(rootView: PermissionHelperAppRowRoot(state: state))
        self.state = state
        self.appRowHost = appRowHost
        host = NSHostingView(rootView: PermissionHelperRoot(state: state, appRowHost: appRowHost))
        super.init(frame: frameRect)
        appRowHost.frame = NSRect(x: 0, y: 0, width: 459, height: 42)
        appRowHost.autoresizingMask = [.width, .height]
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
