/**
 * [INPUT]: 依赖 AppKit/SwiftUI 与 permission-views 的嵌入和前景快照。
 * [OUTPUT]: 提供 AX/Screen 独立行、状态快照与双行视图；保留单卡 Objective-C ABI。
 * [POS]: 双行初始页拥有两个独立呈现状态；行组件只发送注入动作，不读取 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import AppKit
import SwiftUI

func permissionHostDefaultAction(statuses: [String: String], focusedPermission: String?) -> String? {
    let pending = ["accessibility", "screen"].filter { permission in
        statuses[permission] == "not-granted" || statuses[permission] == "unknown"
    }
    if let focusedPermission, pending.contains(focusedPermission) { return focusedPermission }
    return pending.first
}

// --- 可复用行：只持有呈现状态与注入动作，不查询系统权限 ---
@MainActor
final class PermissionRowState: ObservableObject {
    @Published var permissionTitle: String
    @Published var permissionDescription: String
    @Published var statusMessage = ""
    @Published var allow: String
    @Published var completionLabel = "Done"
    @Published var completeInSettings = ""
    @Published var permissionIcon: NSImage?
    @Published var permissionStatus = "unknown"
    @Published var permissionState = "unknown"
    @Published var isDefaultAction = false
    @Published var allowEnabled = true
    @Published var settingsPlaceholder = false
    @Published var placeholderHovered = false
    @Published var layoutDirection: LayoutDirection = .leftToRight
    fileprivate var onAllow: () -> Void
    fileprivate var onResumeSettings: () -> Void
    fileprivate var onFocusChange: (Bool) -> Void = { _ in }

    init(
        title: String = "", description: String = "", buttonTitle: String = "",
        onAllow: @escaping () -> Void = {}, onResumeSettings: @escaping () -> Void = {}
    ) {
        permissionTitle = title
        permissionDescription = description
        allow = buttonTitle
        self.onAllow = onAllow
        self.onResumeSettings = onResumeSettings
    }

    func setContent(allowEnabled: Bool, settingsPlaceholder: Bool) {
        self.allowEnabled = allowEnabled
        self.settingsPlaceholder = settingsPlaceholder
        // 状态切换清除旧占位态的悬停。新指针进入后再建立悬停。
        placeholderHovered = false
    }

    func performAllow() {
        guard allowEnabled, !settingsPlaceholder else { return }
        onAllow()
    }

    func resumeSettings() { onResumeSettings() }
    func setActionFocused(_ focused: Bool) { onFocusChange(focused) }
}

// --- 既有单卡适配：保持 Objective-C 动作与页面状态边界 ---
@MainActor
private final class PermissionInitialState: ObservableObject {
    @Published var title = ""
    @Published var body = ""
    @Published var skip = ""
    @Published var appIcon: NSImage?
    @Published var layoutDirection: LayoutDirection = .leftToRight
    let row: PermissionRowState
    var screenRow: PermissionRowState?
    weak var actionTarget: NSObject?
    private var screenIcon: NSImage?
    private var multiCopy: NSDictionary = [:]
    private var focusedPermission: String?
    private var interactionsEnabled = true

    init() {
        row = PermissionRowState()
        row.onAllow = { [weak self] in self?.send("allow:") }
        row.onResumeSettings = { [weak self] in self?.send("resumeSettings:") }
        row.onFocusChange = { [weak self] focused in self?.setFocused("accessibility", focused: focused) }
        row.isDefaultAction = true
    }

    func configure(
        copy: NSDictionary,
        appIcon: NSImage?,
        permissionIcon: NSImage?,
        actionTarget: NSObject?,
    ) {
        screenRow = nil
        screenIcon = nil
        multiCopy = [:]
        focusedPermission = nil
        interactionsEnabled = true
        row.permissionStatus = "unknown"
        row.permissionState = "unknown"
        row.statusMessage = ""
        row.isDefaultAction = true
        title = permissionCopyString(copy, "title")
        body = permissionCopyString(copy, "body")
        row.permissionTitle = permissionCopyString(copy, "permissionTitle")
        row.permissionDescription = permissionCopyString(copy, "permissionDescription")
        row.allow = permissionCopyString(copy, "repair")
        skip = permissionCopyString(copy, "later")
        row.completeInSettings = permissionCopyString(copy, "completeInSettings")
        self.appIcon = appIcon
        row.permissionIcon = permissionIcon
        self.actionTarget = actionTarget
        layoutDirection = permissionCopyString(copy, "layoutDirection") == "rightToLeft" ? .rightToLeft : .leftToRight
        row.layoutDirection = layoutDirection
        row.setContent(allowEnabled: true, settingsPlaceholder: false)
    }

    func configurePermissions(
        copy: NSDictionary,
        appIcon: NSImage?,
        accessibilityIcon: NSImage?,
        screenIcon: NSImage?,
        actionTarget: NSObject?,
    ) {
        title = permissionCopyString(copy, "title")
        body = permissionCopyString(copy, "body")
        skip = permissionCopyString(copy, "later")
        self.appIcon = appIcon
        self.actionTarget = actionTarget
        layoutDirection = permissionCopyString(copy, "layoutDirection") == "rightToLeft" ? .rightToLeft : .leftToRight
        multiCopy = copy.copy() as? NSDictionary ?? copy
        self.screenIcon = screenIcon
        focusedPermission = nil
        interactionsEnabled = true

        row.permissionTitle = permissionCopyString(copy, "permissionTitle")
        row.permissionDescription = permissionCopyString(copy, "permissionDescription")
        row.statusMessage = row.permissionDescription
        row.allow = permissionCopyString(copy, "repair")
        row.completionLabel = permissionCopyString(copy, "done")
        row.completeInSettings = permissionCopyString(copy, "completeInSettings")
        row.permissionIcon = accessibilityIcon
        row.permissionStatus = "unknown"
        row.permissionState = "unknown"
        row.layoutDirection = layoutDirection
        row.setContent(allowEnabled: true, settingsPlaceholder: false)
        row.onAllow = { [weak self] in self?.send("allow:") }
        row.onResumeSettings = { [weak self] in self?.send("resumeSettings:") }
        row.onFocusChange = { [weak self] focused in self?.setFocused("accessibility", focused: focused) }

        let screenRow = self.screenRow ?? PermissionRowState()
        screenRow.permissionTitle = permissionCopyString(copy, "screenTitle")
        screenRow.permissionDescription = permissionCopyString(copy, "screenDescription")
        screenRow.statusMessage = permissionCopyString(copy, "screenUnknown")
        screenRow.allow = permissionCopyString(copy, "repair")
        screenRow.completionLabel = permissionCopyString(copy, "done")
        screenRow.completeInSettings = permissionCopyString(copy, "screenAddedTitle")
        screenRow.permissionIcon = screenIcon
        screenRow.permissionStatus = "unknown"
        screenRow.permissionState = "unknown"
        screenRow.layoutDirection = layoutDirection
        screenRow.setContent(allowEnabled: true, settingsPlaceholder: false)
        screenRow.onAllow = { [weak self] in self?.send("allowScreen:") }
        screenRow.onResumeSettings = { [weak self] in self?.send("resumeScreenSettings:") }
        screenRow.onFocusChange = { [weak self] focused in self?.setFocused("screen", focused: focused) }
        self.screenRow = screenRow
        refreshDefaultAction()
    }

    func setPermissionState(_ nextState: String, permission: String, message: String? = nil) {
        guard let row = row(for: permission) else { return }
        let isScreen = permission == "screen"
        let descriptionKey = isScreen ? "screenDescription" : "permissionDescription"
        let baseDescription = permissionCopyString(multiCopy, descriptionKey)
        let defaultMessage: String

        if ["repairing", "awaiting-user", "error"].contains(nextState) {
            interactionsEnabled = false
        }

        switch nextState {
        case "granted":
            row.permissionStatus = "granted"
            defaultMessage = baseDescription
            row.setContent(allowEnabled: false, settingsPlaceholder: false)
        case "not-granted":
            row.permissionStatus = "not-granted"
            defaultMessage = baseDescription
            row.setContent(allowEnabled: true, settingsPlaceholder: false)
        case "unknown":
            row.permissionStatus = "unknown"
            defaultMessage = isScreen ? permissionCopyString(multiCopy, "screenUnknown") : baseDescription
            row.setContent(allowEnabled: true, settingsPlaceholder: false)
        case "repairing":
            defaultMessage = permissionCopyString(multiCopy, isScreen ? "screenChecking" : "checking")
            row.setContent(allowEnabled: false, settingsPlaceholder: false)
        case "awaiting-user":
            defaultMessage = permissionCopyString(multiCopy, isScreen ? "screenAddedBody" : "addedBody")
            row.setContent(allowEnabled: false, settingsPlaceholder: true)
        case "error":
            defaultMessage = permissionCopyString(multiCopy, "errorBody")
            row.setContent(allowEnabled: false, settingsPlaceholder: false)
        default:
            return
        }
        row.permissionState = nextState
        row.statusMessage = message.flatMap { $0.isEmpty ? nil : $0 } ?? defaultMessage
        refreshInteractions()
    }

    func setPermissionStatus(_ status: String, permission: String) {
        guard let row = row(for: permission) else { return }
        let normalized = ["granted", "not-granted", "unknown"].contains(status) ? status : "unknown"
        row.permissionStatus = normalized
        guard !["repairing", "awaiting-user", "error"].contains(row.permissionState) else {
            refreshDefaultAction()
            return
        }
        row.permissionState = normalized
        let isScreen = permission == "screen"
        row.statusMessage = isScreen && normalized == "unknown"
            ? permissionCopyString(multiCopy, "screenUnknown")
            : permissionCopyString(multiCopy, isScreen ? "screenDescription" : "permissionDescription")
        row.setContent(allowEnabled: normalized != "granted", settingsPlaceholder: false)
        refreshInteractions()
    }

    func setInteractionsEnabled(_ enabled: Bool) {
        interactionsEnabled = enabled
        refreshInteractions()
    }

    func row(for permission: String) -> PermissionRowState? {
        switch permission {
        case "accessibility": return row
        case "screen": return screenRow
        default: return nil
        }
    }

    func setContent(
        title: String,
        body: String,
        allowEnabled: Bool,
        settingsPlaceholder: Bool,
    ) {
        self.title = title
        self.body = body
        row.setContent(allowEnabled: allowEnabled, settingsPlaceholder: settingsPlaceholder)
    }

    private func setFocused(_ permission: String, focused: Bool) {
        if focused {
            focusedPermission = permission
        } else if focusedPermission == permission {
            focusedPermission = nil
        }
        refreshDefaultAction()
    }

    private func refreshDefaultAction() {
        let statuses = [
            "accessibility": row.allowEnabled && !row.settingsPlaceholder ? row.permissionStatus : "busy",
            "screen": screenRow.map {
                $0.allowEnabled && !$0.settingsPlaceholder ? $0.permissionStatus : "busy"
            } ?? "busy",
        ]
        let selected = permissionHostDefaultAction(statuses: statuses, focusedPermission: focusedPermission)
        row.isDefaultAction = selected == "accessibility"
        screenRow?.isDefaultAction = selected == "screen"
    }

    private func refreshInteractions() {
        for row in [row, screenRow].compactMap({ $0 }) {
            let busy = ["repairing", "awaiting-user", "error"].contains(row.permissionState)
            row.allowEnabled = interactionsEnabled && !busy
                && row.permissionStatus != "granted" && !row.settingsPlaceholder
        }
        refreshDefaultAction()
    }

    func send(_ selector: String) {
        precondition(Thread.isMainThread, "Permission actions must run on the main thread")
        guard let actionTarget else { return }
        _ = NSApp.sendAction(Selector(selector), to: actionTarget, from: nil)
    }
}

private struct PermissionAllowButtonStyle: ButtonStyle {
    @Environment(\.isEnabled) private var isEnabled
    @Environment(\.controlActiveState) private var controlActiveState

    func makeBody(configuration: Configuration) -> some View {
        PermissionAllowButtonFace(
            label: configuration.label,
            isEnabled: isEnabled,
            isActive: controlActiveState == .key,
            isPressed: configuration.isPressed
        )
    }
}

private struct PermissionAllowButtonFace<Label: View>: View {
    let label: Label
    let isEnabled: Bool
    let isActive: Bool
    let isPressed: Bool

    private var foreground: Color {
        guard isEnabled else { return Color(nsColor: .disabledControlTextColor) }
        return isActive ? .white : Color(nsColor: .controlTextColor)
    }

    private var background: Color {
        guard isEnabled else { return Color(nsColor: .controlColor) }
        guard !isActive else { return .accentColor }
        if #available(macOS 14.0, *) { return Color(nsColor: .systemFill) }
        return Color.primary.opacity(0.08)
    }

    var body: some View {
        label
            // 4pt was the measured label adjustment under the automatic
            // style; replace its lost 8pt-per-side system inset explicitly.
            .padding(.horizontal, 12)
            .frame(height: 24)
            .foregroundStyle(foreground)
            .background {
                Capsule(style: .continuous)
                    .fill(background)
                    // Original Allow has no visible hover treatment. Its
                    // pressed fill darkens in place, without another surface.
                    .brightness(isPressed ? -0.115 : 0)
            }
            .animation(.easeOut(duration: 0.08), value: isPressed)
    }
}

private struct PermissionDefaultActionShortcut: ViewModifier {
    let enabled: Bool

    @ViewBuilder
    func body(content: Content) -> some View {
        if enabled {
            content.keyboardShortcut(.defaultAction)
        } else {
            content
        }
    }
}

struct PermissionCardRoot: View {
    @ObservedObject var state: PermissionRowState
    @Environment(\.colorScheme) private var colorScheme
    @FocusState private var allowFocused: Bool

    var foreground: some View {
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
                Text(state.statusMessage.isEmpty ? state.permissionDescription : state.statusMessage)
                    .font(.system(size: 13))
                    .foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            if state.permissionStatus == "granted" {
                Text("\(state.completionLabel) ✓")
                    .font(.system(size: 13))
                    .foregroundStyle(.secondary)
                    .frame(minWidth: 62)
                    .offset(x: state.layoutDirection == .rightToLeft ? -4 : 4)
            } else {
                Button { state.performAllow() } label: {
                    Text(state.allow)
                }
                    .buttonStyle(PermissionAllowButtonStyle())
                    .modifier(PermissionDefaultActionShortcut(enabled: state.isDefaultAction))
                    .focused($allowFocused)
                    .onChange(of: allowFocused) { focused in state.setActionFocused(focused) }
                    .clipShape(Capsule(style: .continuous))
                    .frame(minWidth: 62)
                    .offset(x: state.layoutDirection == .rightToLeft ? -4 : 4)
                    .disabled(!state.allowEnabled)
            }
        }
        .padding(.trailing, 20)
        .frame(width: 518, height: 80)
    }

    var body: some View {
        foreground
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
    @ObservedObject var state: PermissionRowState

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
            .scaleEffect(pressed ? 0.99 : 1)
            .opacity(pressed ? 0.88 : 1)
            .contentShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
            .onHover { state.placeholderHovered = $0 }
    }
}

private func permissionPlaceholderText(_ value: String) -> Text {
    let attributed = NSAttributedString(
        string: value,
        attributes: [
            .font: NSFont.systemFont(ofSize: 12, weight: .medium),
            .kern: 0.7,
        ],
    )
    return Text(AttributedString(attributed))
}

private struct PermissionPlaceholderButtonStyle: ButtonStyle {
    let state: PermissionRowState

    func makeBody(configuration: Configuration) -> some View {
        PermissionPlaceholderLabel(
            label: configuration.label,
            pressed: configuration.isPressed,
            state: state,
        )
    }
}

private struct PermissionInitialPermissionRow: View {
    @ObservedObject var row: PermissionRowState
    let cardHost: NSView

    var body: some View {
        ZStack {
            PermissionEmbeddedView(view: cardHost, size: CGSize(width: 518, height: 80))
            if row.settingsPlaceholder {
                Button { row.resumeSettings() } label: {
                    permissionPlaceholderText(row.completeInSettings)
                        .opacity(0.86)
                }
                .buttonStyle(PermissionPlaceholderButtonStyle(state: row))
            }
        }
        .frame(width: 518, height: 80)
    }
}

private struct PermissionInitialRoot: View {
    @ObservedObject var state: PermissionInitialState
    let cardHost: NSView
    let screenCardHost: NSView?

    var body: some View {
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

            Group {
                if let screenRow = state.screenRow, let screenCardHost {
                    VStack(spacing: 18) {
                        PermissionInitialPermissionRow(row: state.row, cardHost: cardHost)
                        PermissionInitialPermissionRow(row: screenRow, cardHost: screenCardHost)
                    }
                    .frame(width: 518)
                } else {
                    PermissionInitialPermissionRow(row: state.row, cardHost: cardHost)
                }
            }
            .padding(.top, 21)
        }
        .frame(width: 600)
        .offset(y: -9)
        .padding(.bottom, 32)
        .background(.regularMaterial)
        .overlay(alignment: .bottomTrailing) {
            if !state.row.settingsPlaceholder && !(state.screenRow?.settingsPlaceholder ?? false) {
                Button(state.skip) { state.send("skip:") }
                    .buttonStyle(.plain)
                    .frame(height: 41, alignment: .center)
                    .padding(.trailing, 57)
            }
        }
        .environment(\.layoutDirection, state.layoutDirection)
    }
}

@MainActor
@objc(IncodexPermissionInitialView)
public final class IncodexPermissionInitialView: NSView {
    private let state: PermissionInitialState
    private let host: NSHostingView<PermissionInitialRoot>
    private let cardHost: NSHostingView<PermissionCardRoot>
    private var screenCardHost: NSHostingView<PermissionCardRoot>?

    @objc public var permissionCardView: NSView { cardHost }

    public func permissionCardView(forPermission permission: String) -> NSView? {
        switch permission {
        case "accessibility": return cardHost
        case "screen": return screenCardHost
        default: return nil
        }
    }

    @objc(snapshotPermissionCardWithScale:)
    public func snapshotPermissionCard(scale: CGFloat) -> NSImage? {
        precondition(Thread.isMainThread, "Permission snapshots must run on the main thread")
        return snapshotPermissionCard(row: state.row, host: cardHost, scale: scale)
    }

    @objc(snapshotPermissionCardWithScale:permission:)
    public func snapshotPermissionCard(scale: CGFloat, permission: String) -> NSImage? {
        precondition(Thread.isMainThread, "Permission snapshots must run on the main thread")
        guard let row = state.row(for: permission),
              let host = permission == "screen" ? screenCardHost : cardHost else { return nil }
        return snapshotPermissionCard(row: row, host: host, scale: scale)
    }

    @objc(configurePermissionsWithCopy:appIcon:accessibilityIcon:screenIcon:actionTarget:)
    public func configurePermissions(
        copy: NSDictionary,
        appIcon: NSImage?,
        accessibilityIcon: NSImage?,
        screenIcon: NSImage?,
        actionTarget: NSObject?,
    ) {
        precondition(Thread.isMainThread, "Permission configuration must run on the main thread")
        state.configurePermissions(
            copy: copy,
            appIcon: appIcon,
            accessibilityIcon: accessibilityIcon,
            screenIcon: screenIcon,
            actionTarget: actionTarget,
        )
        guard let screenRow = state.screenRow else { return }
        let screenCardHost = self.screenCardHost ?? NSHostingView(rootView: PermissionCardRoot(state: screenRow))
        self.screenCardHost = screenCardHost
        screenCardHost.frame = NSRect(x: 0, y: 0, width: 518, height: 80)
        screenCardHost.autoresizingMask = [.width, .height]
        host.rootView = PermissionInitialRoot(
            state: state,
            cardHost: cardHost,
            screenCardHost: screenCardHost,
        )
        host.needsLayout = true
        host.layoutSubtreeIfNeeded()
    }

    func setPermissionState(_ nextState: String, permission: String, message: String? = nil) {
        precondition(Thread.isMainThread, "Permission state must change on the main thread")
        guard state.screenRow != nil else { return }
        state.setPermissionState(nextState, permission: permission, message: message)
        refreshDualRoot()
    }

    func setPermissionStatus(_ status: String, permission: String) {
        precondition(Thread.isMainThread, "Permission status must change on the main thread")
        guard state.screenRow != nil else { return }
        state.setPermissionStatus(status, permission: permission)
        refreshDualRoot()
    }

    func setPermissionInteractionsEnabled(_ enabled: Bool) {
        precondition(Thread.isMainThread, "Permission actions must change on the main thread")
        guard state.screenRow != nil else { return }
        state.setInteractionsEnabled(enabled)
        refreshDualRoot()
    }

    private func refreshDualRoot() {
        guard state.screenRow != nil else { return }
        cardHost.isHidden = state.row.settingsPlaceholder
        screenCardHost?.isHidden = state.screenRow?.settingsPlaceholder ?? false
        host.rootView = PermissionInitialRoot(
            state: state,
            cardHost: cardHost,
            screenCardHost: screenCardHost,
        )
        host.needsLayout = true
        host.layoutSubtreeIfNeeded()
    }

    @objc public var preferredContentSize: NSSize {
        precondition(Thread.isMainThread, "Permission layout must run on the main thread")
        let measured = host.fittingSize
        return NSSize(width: 600, height: measured.height)
    }

    public override var isFlipped: Bool { true }

    public override init(frame frameRect: NSRect) {
        precondition(Thread.isMainThread, "Permission views must be created on the main thread")
        let state = PermissionInitialState()
        let cardHost = NSHostingView(rootView: PermissionCardRoot(state: state.row))
        self.state = state
        self.cardHost = cardHost
        host = NSHostingView(rootView: PermissionInitialRoot(state: state, cardHost: cardHost, screenCardHost: nil))
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
        cardHost.isHidden = false
        screenCardHost?.isHidden = false
        host.rootView = PermissionInitialRoot(state: state, cardHost: cardHost, screenCardHost: nil)
        host.needsLayout = true
        host.layoutSubtreeIfNeeded()
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
        // The Runtime measures immediately after this selector returns. Reassign
        // the same root value synchronously so a @Published update cannot leave
        // fittingSize one body revision behind; cardHost remains the stable
        // embedded NSHostingView used by the snapshot/placeholder bridge.
        host.rootView = PermissionInitialRoot(state: state, cardHost: cardHost, screenCardHost: nil)
        host.needsLayout = true
        host.layoutSubtreeIfNeeded()
    }

    private func snapshotPermissionCard(
        row: PermissionRowState,
        host: NSHostingView<PermissionCardRoot>,
        scale: CGFloat,
    ) -> NSImage? {
        let colorScheme: ColorScheme = effectiveAppearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua
            ? .dark : .light
        let foreground = PermissionCardRoot(state: row).foreground
            .environment(\.layoutDirection, row.layoutDirection)
            .environment(\.colorScheme, colorScheme)
        return permissionForegroundSnapshot(
            content: foreground, size: host.bounds.size, scale: scale,
            appearance: effectiveAppearance, useHostingView: true
        )
    }
}
