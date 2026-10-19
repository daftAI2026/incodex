/**
 * [INPUT]: 依赖共用系统呈现、SwiftUI 视图和 CLI 事件回调。
 * [OUTPUT]: 提供单卡兼容与 AX/Screen 双行授权 Presenter。
 * [POS]: Presenter 管窗口交易；CLI 独占权限快照和全局完成裁决。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import AppKit
import Foundation

// The SwiftUI views in permission-views.swift are the renderer.  This file is
// deliberately limited to the short-lived AppKit presenter: panel lifetime,
// screen/window geometry, drag tracking, and the native guide state machine.
// TCC inspection, reset, and Settings launching remain in the CLI process.

@MainActor
public final class PermissionHostPresenter: NSObject {
    let copy: NSDictionary
    let layoutDirection: String
    let screenVerification: String
    let onEvent: (String) -> Void
    let onError: (String) -> Void
    lazy var actionTarget = PermissionHostActionTarget(owner: self)

    var initialPanel: NSWindow?
    var initialView: IncodexPermissionInitialView?
    var cardView: NSView?
    var helperPanel: NSPanel?
    var helperView: IncodexPermissionHelperView?
    var appRowView: NSView?
    var dragView: PermissionHostDragView?
    var dragSession: NSDraggingSession?
    var terminalDragPanel: NSPanel?
    var terminalDragSession: NSDraggingSession?
    var arrowPanel: NSPanel?
    var arrowView: IncodexPermissionArrowView?
    var arrowTracker: PermissionHostArrowTrackerView?
    var initialDelegate: PermissionHostWindowDelegate?
    var helperDelegate: PermissionHostWindowDelegate?
    var arrowDelegate: PermissionHostWindowDelegate?
    var trackingTimer: Timer?
    private var arrowTimer: Timer?
    private var arrowReturnTimer: Timer?
    private var backFlightTimer: Timer?
    private var flight: PermissionHostFlight?
    var sourceEndpoint: PermissionHostFlightEndpoint?
    var permissionsConfigured = false
    var permissionStatuses = ["accessibility": "unknown", "screen": "unknown"]
    var requestedPermissions: Set<String> = []
    var retryReadyPermissions: Set<String> = []
    var activePermission = "accessibility"
    var state = "pending"
    var closed = false
    var presented = false
    var settled = false
    var retryReady = false
    var returning = false
    var dragging = false
    var locatingAttempts = 0
    private var forwardSequence = 0
    private var returnSequence = 0

    public init(
        copy: NSDictionary,
        layoutDirection: String,
        screenVerification: String = "probe",
        onEvent: @escaping (String) -> Void,
        onError: @escaping (String) -> Void,
    ) {
        self.copy = IncodexPermissionPlatform.copyForCurrentSystem(copy).copy() as? NSDictionary ?? copy
        self.layoutDirection = layoutDirection == "rightToLeft" ? "rightToLeft" : "leftToRight"
        self.screenVerification = screenVerification == "manual" ? "manual" : "probe"
        self.onEvent = onEvent
        self.onError = onError
        super.init()
    }

    public func configurePermissions(_ statuses: [String: String]) {
        guard !presented, !closed else {
            reportMessage("permissions must be configured before presentation")
            return
        }
        permissionsConfigured = true
        permissionStatuses = normalizedPermissionStatuses(statuses)
        requestedPermissions = []
        retryReadyPermissions = []
        state = "pending"
        activePermission = permissionHostDefaultAction(
            statuses: permissionStatuses,
            focusedPermission: nil,
        ) ?? "accessibility"
    }

    public func updatePermissions(_ statuses: [String: String]) {
        guard permissionsConfigured, !closed else { return }
        let previous = permissionStatuses
        let current = normalizedPermissionStatuses(statuses)
        permissionStatuses = current
        for permission in ["accessibility", "screen"] {
            let status = current[permission] ?? "unknown"
            if previous[permission] == "granted" && status != "granted" {
                requestedPermissions.remove(permission)
                retryReadyPermissions.remove(permission)
            }
            if status == "granted" {
                initialView?.setPermissionState("granted", permission: permission)
                if activePermission == permission && isPermissionTransactionActive {
                    if helperPanel != nil || state != "pending" {
                        restoreInitialPage(makeActivePermissionRetryable: false)
                    }
                }
            } else if permission != activePermission || !isPermissionTransactionActive {
                initialView?.setPermissionStatus(status, permission: permission)
            }
        }
    }

    public func setPermissionState(_ nextState: String, permission: String, message: String? = nil) {
        guard permissionsConfigured, !closed else { return }
        guard permission == "accessibility" || permission == "screen" else {
            reportMessage("unsupported native permission: \(permission)")
            return
        }
        guard ["granted", "not-granted", "unknown", "repairing", "awaiting-user", "error"].contains(nextState) else {
            reportMessage("unsupported native permission state: \(nextState)")
            return
        }

        let previousStatus = permissionStatuses[permission]
        if ["granted", "not-granted", "unknown"].contains(nextState) {
            permissionStatuses[permission] = nextState
            if previousStatus == "granted" && nextState != "granted" {
                requestedPermissions.remove(permission)
                retryReadyPermissions.remove(permission)
            }
        }
        initialView?.setPermissionState(nextState, permission: permission, message: message)

        switch nextState {
        case "repairing":
            activePermission = permission
            state = nextState
            cardView = initialView?.permissionCardView(forPermission: permission) ?? cardView
            initialView?.setPermissionInteractionsEnabled(false)
            initialPanel?.level = .normal
            stopTracking()
        case "awaiting-user":
            activePermission = permission
            state = nextState
            cardView = initialView?.permissionCardView(forPermission: permission) ?? cardView
            initialView?.setPermissionInteractionsEnabled(false)
            initialPanel?.level = .normal
            cardView?.isHidden = true
            if helperPanel == nil { SettingsLocator().prepareHandoff() }
            startSettingsTracking()
        case "error":
            activePermission = permission
            state = nextState
            cardView = initialView?.permissionCardView(forPermission: permission) ?? cardView
            initialView?.setPermissionInteractionsEnabled(false)
            stopTracking()
            stopBackFlightTimer()
            disposeActiveFlight()
            disposeHelper()
            cardView?.isHidden = false
            initialPanel?.level = .floating
            fitInitialPage()
            initialPanel?.orderFront(nil)
        case "granted":
            requestedPermissions.insert(permission)
            retryReadyPermissions.remove(permission)
            if helperPanel != nil && activePermission == permission {
                restoreInitialPage(makeActivePermissionRetryable: false)
            } else if activePermission == permission {
                resetPermissionTransaction(permission: permission)
            }
        case "not-granted", "unknown":
            if !isPermissionTransactionActive {
                state = "pending"
                initialView?.setPermissionInteractionsEnabled(true)
            }
        default:
            break
        }
    }

    public var canStartDrag: Bool {
        !closed && !dragging && state == "awaiting-user" && helperPanel != nil
    }

    private var permissionsAreGranted: Bool {
        permissionStatuses["accessibility"] == "granted" && permissionStatuses["screen"] == "granted"
    }

    private var isPermissionTransactionActive: Bool {
        ["requesting", "repairing", "awaiting-user", "error"].contains(state)
    }

    private func normalizedPermissionStatuses(_ statuses: [String: String]) -> [String: String] {
        [
            "accessibility": normalizedPermissionStatus(statuses["accessibility"] ?? "unknown"),
            "screen": normalizedPermissionStatus(statuses["screen"] ?? "unknown"),
        ]
    }

    private func normalizedPermissionStatus(_ status: String) -> String {
        ["granted", "not-granted", "unknown"].contains(status) ? status : "unknown"
    }

    private func resetPermissionTransaction(permission: String) {
        retryReadyPermissions.remove(permission)
        guard presented else {
            state = "pending"
            activePermission = permissionHostDefaultAction(
                statuses: permissionStatuses,
                focusedPermission: nil,
            ) ?? permission
            return
        }
        restoreInitialPage(makeActivePermissionRetryable: false)
    }

    @discardableResult
    public func present() -> Bool {
        guard !closed, !presented else { return !closed }
        if permissionsConfigured && permissionsAreGranted {
            close()
            return true
        }
        guard let appIcon = permissionHostAppIcon() else {
            reportMessage("ChatGPT icon is unavailable")
            return false
        }
        _ = NSApplication.shared
            let panel = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: permissionHostInitialWidth, height: 312),
                styleMask: [.titled, .closable, .fullSizeContentView],
                backing: .buffered,
                defer: false,
            )
            panel.isReleasedWhenClosed = false
            panel.hidesOnDeactivate = false
            panel.level = NSWindow.Level(rawValue: 3)
            panel.title = permissionHostString(self.copy, "title")
            panel.titleVisibility = .hidden
            panel.titlebarAppearsTransparent = true
            panel.isMovableByWindowBackground = true
            panel.isOpaque = false
            panel.backgroundColor = NSColor.white.withAlphaComponent(0.001)
            let delegate = PermissionHostWindowDelegate { [weak self] in self?.handleWindowClosed() }
            panel.delegate = delegate

            let view = IncodexPermissionInitialView(frame: panel.contentView?.bounds ?? NSRect(x: 0, y: 0, width: permissionHostInitialWidth, height: 312))
            let nativeCopy = permissionHostNativeCopy(self.copy, layoutDirection: layoutDirection)
            let card: NSView
            if permissionsConfigured {
                let accessibilityIcon = permissionHostPermissionIcon(title: permissionHostString(nativeCopy, "permissionTitle"))
                let screenIcon = permissionHostScreenIcon(title: permissionHostString(nativeCopy, "screenTitle"))
                view.configurePermissions(
                    copy: nativeCopy,
                    appIcon: appIcon,
                    accessibilityIcon: accessibilityIcon,
                    screenIcon: screenIcon,
                    actionTarget: actionTarget,
                )
                for permission in ["accessibility", "screen"] {
                    view.setPermissionState(permissionStatuses[permission] ?? "unknown", permission: permission)
                }
                card = view.permissionCardView(forPermission: activePermission) ?? view.permissionCardView
            } else {
                let permissionIcon = permissionHostPermissionIcon(title: permissionHostString(nativeCopy, "permissionTitle"))
                view.configure(
                    copy: nativeCopy,
                    appIcon: appIcon,
                    permissionIcon: permissionIcon,
                    actionTarget: actionTarget,
                )
                view.setContent(
                    title: permissionHostString(nativeCopy, "title"),
                    body: permissionHostString(nativeCopy, "body"),
                    allowEnabled: true,
                    settingsPlaceholder: false,
                )
                card = view.permissionCardView
            }
            panel.contentView = view
            initialPanel = panel
            initialView = view
            cardView = card
            initialDelegate = delegate
            guard fitInitialPage() else {
                panel.delegate = nil
                panel.close()
                initialPanel = nil
                initialView = nil
                cardView = nil
                initialDelegate = nil
                return false
            }
            panel.center()
            presented = true
            NSApplication.shared.activate(ignoringOtherApps: true)
            panel.makeKeyAndOrderFront(nil)
        return true
    }

    public func setState(_ nextState: String, message: String? = nil) {
        guard !closed else { return }
        if permissionsConfigured {
            if nextState == "granted" {
                close()
                return
            }
            setPermissionState(nextState, permission: activePermission, message: message)
            return
        }
        switch nextState {
        case "repairing":
            state = nextState
            initialPanel?.level = .normal
            stopTracking()
            // Match the reference transition: keep the original card until
            // the same-height Settings placeholder takes its place.
        case "awaiting-user":
            let enteringAwaitingUser = state != "awaiting-user"
            state = nextState
            initialPanel?.level = .normal
            setInitialContent(
                title: permissionHostString(copy, "title"),
                body: permissionHostString(copy, "body"),
                allowEnabled: false,
                settingsPlaceholder: true,
            )
            cardView?.isHidden = true
            if enteringAwaitingUser { SettingsLocator().prepareHandoff() }
            startSettingsTracking()
        case "granted":
            close()
        case "error":
            guard setGuideActivationPolicy(.accessory) else { return }
            state = nextState
            returning = false
            stopTracking()
            stopBackFlightTimer()
            disposeActiveFlight()
            disposeHelper()
            initialPanel?.level = .floating
            cardView?.isHidden = false
            setInitialContent(
                title: permissionHostString(copy, "errorTitle"),
                body: permissionHostString(copy, "errorBody"),
                allowEnabled: false,
                settingsPlaceholder: false,
            )
            fitInitialPage()
            initialPanel?.orderFront(nil)
        default:
            reportMessage("unsupported native permission state: \(nextState)")
        }
    }

    public func close() {
        guard !closed else { return }
        // AppKit can reorder the dragging source even after a close request.
        // Keep that source alive, but retire every pixel until ended arrives.
        _ = retainDragSourceUntilEnded()
        closed = true
        presented = false
        returning = false
        retryReady = false
        dragging = false
        dragSession = nil
        stopTracking()
        stopArrow()
        stopBackFlightTimer()
        disposeActiveFlight()
        if helperPanel !== terminalDragPanel { dragView?.removeFromSuperview() }
        dragView = nil

        if let arrowPanel {
            helperPanel?.removeChildWindow(arrowPanel)
            arrowPanel.delegate = nil
            arrowPanel.orderOut(nil)
            arrowPanel.close()
        }
        arrowPanel = nil
        arrowView = nil
        arrowTracker = nil

        if let helperPanel {
            helperPanel.delegate = nil
            helperPanel.orderOut(nil)
            if helperPanel !== terminalDragPanel { helperPanel.close() }
        }
        helperPanel = nil
        helperView = nil
        appRowView = nil

        if let initialPanel {
            initialPanel.delegate = nil
            initialPanel.orderOut(nil)
            initialPanel.close()
        }
        initialPanel = nil
        initialView = nil
        cardView = nil
        // Close can arrive directly from the CLI or after a grant without a
        // Back transition. No guide window remains visible at this point.
        if NSApplication.shared.activationPolicy() == .prohibited {
            _ = NSApplication.shared.setActivationPolicy(.accessory)
        }
    }

    func startForwardFlight() {
        guard let sourceEndpoint, let targetEndpoint = helperEndpoint() else {
            revealHelper()
            return
        }
        forwardSequence += 1
        let sequence = forwardSequence
        let active = PermissionHostFlight(
            source: sourceEndpoint,
            target: { [weak self] in self?.helperEndpoint() ?? targetEndpoint },
            reverse: false,
            isClosed: { [weak self] in self?.closed ?? true },
            onComplete: { [weak self] in self?.finishForwardFlight(sequence: sequence) },
            onError: { [weak self] _ in self?.noteFlightFallback() },
        )
        flight = active
        active.start()
    }

    private func finishForwardFlight(sequence: Int) {
        guard !closed, !returning, state == "awaiting-user", forwardSequence == sequence, flight != nil else { return }
        flight = nil
        revealHelper()
    }

    func startBackFlight() {
        guard !closed, !returning, !dragging, state == "awaiting-user",
              let initialView,
              let cardView else { return }
        guard setGuideActivationPolicy(.accessory) else { return }
        returning = true
        returnSequence += 1
        let sequence = returnSequence
        stopTracking()
        stopArrow()
        retryReady = false
        NSApplication.shared.activate(ignoringOtherApps: false)
        setInitialContent(
            title: permissionHostString(copy, "title"),
            body: permissionHostString(copy, "body"),
            allowEnabled: true,
            settingsPlaceholder: true,
        )
        fitInitialPage()
        if NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            fallbackToInitial()
            return
        }
        guard let helperEndpoint = helperEndpoint() else {
            fallbackToInitial()
            return
        }
        cardView.isHidden = false
        guard let targetEndpoint = captureInitialEndpoint() else {
            cardView.isHidden = true
            fallbackToInitial()
            return
        }
        cardView.isHidden = true
        initialPanel?.orderFront(nil)
        // The flight's `reverse` flag swaps the captured source/target image
        // order internally.  Pass the card as the source (it owns the
        // concrete image) and the helper as the destination; this produces a
        // helper-to-card reverse flight without asking the helper endpoint to
        // provide a source image it intentionally does not own.
        let active = PermissionHostFlight(
            source: targetEndpoint,
            target: { [weak self] in self?.helperEndpoint() ?? helperEndpoint },
            reverse: true,
            isClosed: { [weak self] in self?.closed ?? true },
            onComplete: { [weak self] in self?.finishBackFlight(sequence: sequence) },
            onError: { [weak self] _ in self?.noteFlightFallback() },
        )
        flight = active
        helperPanel?.alphaValue = 0
        helperView?.alphaValue = 0
        arrowPanel?.alphaValue = 0
        active.start()
        guard flight === active, returning, !closed else { return }
        arrowPanel?.orderOut(nil)
        backFlightTimer = Timer.scheduledTimer(withTimeInterval: 5, repeats: false) { [weak self, weak active] _ in
            Task { @MainActor [weak self, weak active] in
                guard let self, let active,
                      !self.closed, self.returnSequence == sequence, self.flight === active else { return }
                self.fallbackToInitial()
            }
        }
        _ = initialView
    }

    private func finishBackFlight(sequence: Int) {
        guard !closed, returnSequence == sequence, flight != nil else { return }
        flight = nil
        stopBackFlightTimer()
        restoreInitialPage(raiseBeforeHelperDisposal: true)
    }

    private func fallbackToInitial() {
        returnSequence += 1
        stopBackFlightTimer()
        disposeActiveFlight()
        restoreInitialPage()
    }

    func restoreInitialPage(
        raiseBeforeHelperDisposal: Bool = false,
        makeActivePermissionRetryable: Bool = true,
    ) {
        guard !closed else { return }
        // ---- 回卡前统一退役，防止旧回调重入新状态。 ----
        stopTracking()
        stopArrow()
        stopBackFlightTimer()
        disposeActiveFlight()
        guard setGuideActivationPolicy(.accessory) else { return }
        if raiseBeforeHelperDisposal { initialPanel?.level = .floating }
        disposeHelper()
        returning = false
        state = "pending"
        if permissionsConfigured {
            if makeActivePermissionRetryable && permissionStatuses[activePermission] != "granted" {
                retryReadyPermissions.insert(activePermission)
            } else {
                retryReadyPermissions.remove(activePermission)
            }
            let selected = permissionHostDefaultAction(statuses: permissionStatuses, focusedPermission: nil)
            if !makeActivePermissionRetryable, let selected { activePermission = selected }
            initialView?.setPermissionState(
                permissionStatuses[activePermission] ?? "unknown",
                permission: activePermission,
            )
            initialView?.setPermissionInteractionsEnabled(true)
            cardView = initialView?.permissionCardView(forPermission: activePermission)
            cardView?.isHidden = false
        } else {
            retryReady = true
            setInitialContent(
                title: permissionHostString(copy, "title"),
                body: permissionHostString(copy, "body"),
                allowEnabled: true,
                settingsPlaceholder: false,
            )
        }
        fitInitialPage()
        if !raiseBeforeHelperDisposal { initialPanel?.level = .floating }
        NSApplication.shared.activate(ignoringOtherApps: true)
        initialPanel?.makeKeyAndOrderFront(nil)
    }

    private func disposeActiveFlight() {
        // Clear the strong reference before disposing.  PermissionHostFlight
        // may synchronously call onComplete/onError during dispose; clearing
        // first prevents a stale callback from re-showing a closed page or
        // from entering the next state transition.
        let active = flight
        flight = nil
        active?.dispose()
    }

    private func noteFlightFallback() {
        // A replica failure is visual-only: dispose settles its completion
        // callback, which reveals the helper or restores the initial card.
        // The controller must not treat it as a permission transaction error.
        NSLog("permission flight fallback")
    }

    private func disposeHelper() {
        let retainsDragSource = retainDragSourceUntilEnded()
        stopArrow()
        if !retainsDragSource { dragView?.removeFromSuperview() }
        dragView = nil
        if let arrowPanel {
            helperPanel?.removeChildWindow(arrowPanel)
            arrowPanel.delegate = nil
            arrowPanel.orderOut(nil)
            arrowPanel.close()
        }
        if let helperPanel {
            helperPanel.delegate = nil
            helperPanel.orderOut(nil)
            if helperPanel !== terminalDragPanel { helperPanel.close() }
        }
        arrowPanel = nil
        arrowView = nil
        arrowTracker = nil
        helperPanel = nil
        helperView = nil
        appRowView = nil
        helperDelegate = nil
        arrowDelegate = nil
        presented = false
        if !retainsDragSource { dragging = false }
    }

    @discardableResult
    private func retainDragSourceUntilEnded() -> Bool {
        if terminalDragPanel != nil { return true }
        guard dragging || dragSession != nil, let helperPanel else { return false }
        terminalDragPanel = helperPanel
        terminalDragSession = dragSession
        helperPanel.alphaValue = 0
        return true
    }

    private func stopBackFlightTimer() {
        backFlightTimer?.invalidate()
        backFlightTimer = nil
    }

    func scheduleArrow(after delay: TimeInterval) {
        stopArrow()
        guard !closed else { return }
        if NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            arrowView?.resetToIdentity()
            return
        }
        arrowTimer = Timer.scheduledTimer(withTimeInterval: delay, repeats: false) { [weak self] _ in
            Task { @MainActor [weak self] in
                self?.stretchArrow()
            }
        }
    }

    func stretchArrow() {
        stopArrow()
        guard !closed, presented else { return }
        if NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            arrowView?.resetToIdentity()
            return
        }
        if dragging {
            scheduleArrow(after: 4)
            return
        }
        arrowView?.animate(toScaleX: 1.15, scaleY: 1.6)
        arrowReturnTimer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: false) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self, !self.closed, !self.dragging else { return }
                if NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
                    self.arrowView?.resetToIdentity()
                    return
                }
                self.arrowView?.animate(toScaleX: 1, scaleY: 1)
                self.scheduleArrow(after: 4)
            }
        }
    }

    func stopArrow() {
        arrowTimer?.invalidate()
        arrowTimer = nil
        arrowReturnTimer?.invalidate()
        arrowReturnTimer = nil
    }
}
