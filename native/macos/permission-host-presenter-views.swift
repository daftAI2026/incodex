/**
 * [INPUT]: 依赖系统权限图标、录屏 UTType、SettingsLocator 与 Presenter 状态。
 * [OUTPUT]: 提供权限 action、helper/arrow/drag 和 Settings 跟踪。
 * [POS]: 拖放收据须命中当前前台 Settings；双行 helper 暂失只等待，legacy 单行十次 miss 发 Later。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import AppKit
import Foundation
import UniformTypeIdentifiers

private let permissionHostAppPath = "/Applications/ChatGPT.app"

@MainActor
func permissionHostAppIcon() -> NSImage? {
    NSImage(contentsOfFile: "\(permissionHostAppPath)/Contents/Resources/icon-chatgpt.png")
}

@MainActor
func permissionHostPermissionIcon(title: String) -> NSImage? {
    IncodexPermissionPlatform.accessibilityIcon()
}

@MainActor
func permissionHostScreenIcon(title: String) -> NSImage? {
    if let screenType = UTType("com.apple.graphic-icon.screen-recording") {
        return NSWorkspace.shared.icon(for: screenType)
    }
    return NSImage(systemSymbolName: "record.circle", accessibilityDescription: title)
        ?? NSImage(systemSymbolName: "display", accessibilityDescription: title)
        ?? permissionHostAppIcon()
}

func permissionHostString(_ copy: NSDictionary, _ key: String) -> String {
    if let value = copy[key] as? String { return value }
    if let value = copy[key] as? NSString { return value as String }
    return ""
}

func permissionHostNativeCopy(_ copy: NSDictionary, layoutDirection: String) -> NSDictionary {
    let keys = [
        "title", "body", "permissionTitle", "permissionDescription", "repair", "later", "back",
        "addedTitle", "addedBody", "dragInstruction", "dragInstructionRuns", "completeInSettings",
        "checking", "repairing", "openSettings", "errorTitle", "errorBody",
        "screenTitle", "screenDescription", "screenAddedTitle", "screenAddedBody",
        "screenDragInstruction", "screenDragInstructionRuns", "screenChecking", "done", "screenUnknown",
    ]
    let result = NSMutableDictionary(capacity: keys.count + 1)
    for key in keys { result[key] = permissionHostString(copy, key) }
    result["layoutDirection"] = layoutDirection == "rightToLeft" ? "rightToLeft" : "leftToRight"
    return result
}

@MainActor
final class PermissionHostWindowDelegate: NSObject, NSWindowDelegate {
    let onClose: () -> Void

    init(onClose: @escaping () -> Void) {
        self.onClose = onClose
        super.init()
    }

    func windowWillClose(_ notification: Notification) { onClose() }
}

@MainActor
final class PermissionHostActionTarget: NSObject {
    weak var owner: PermissionHostPresenter?

    init(owner: PermissionHostPresenter) {
        self.owner = owner
        super.init()
    }

    @objc(allow:)
    func allow(_ sender: Any?) { owner?.handleAllow(permission: "accessibility") }

    @objc(allowScreen:)
    func allowScreen(_ sender: Any?) { owner?.handleAllow(permission: "screen") }

    @objc(skip:)
    func skip(_ sender: Any?) { owner?.handleSkip() }

    @objc(resumeSettings:)
    func resumeSettings(_ sender: Any?) { owner?.handleResumeSettings(permission: "accessibility") }

    @objc(resumeScreenSettings:)
    func resumeScreenSettings(_ sender: Any?) { owner?.handleResumeSettings(permission: "screen") }

    @objc(later:)
    func later(_ sender: Any?) { owner?.handleLater() }
}

@MainActor
final class PermissionHostHelperPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

@MainActor
final class PermissionHostArrowPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

@MainActor
final class PermissionHostArrowTrackerView: NSView {
    weak var owner: PermissionHostPresenter?

    override func mouseEntered(with event: NSEvent) { owner?.handleArrowEntered() }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        for area in trackingAreas { removeTrackingArea(area) }
        addTrackingArea(
            NSTrackingArea(
                rect: bounds,
                options: [.mouseEnteredAndExited, .activeAlways, .inVisibleRect],
                owner: self,
                userInfo: nil,
            )
        )
    }
}

@MainActor
final class PermissionHostDragView: NSView, NSDraggingSource, NSPasteboardItemDataProvider {
    weak var owner: PermissionHostPresenter?
    weak var rowView: NSView?

    override var isFlipped: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func mouseDown(with event: NSEvent) {
        guard let owner, owner.canStartDrag, let rowView else { return }
        let item = NSPasteboardItem()
        item.setDataProvider(self, forTypes: [.fileURL])
        let draggingItem = NSDraggingItem(pasteboardWriter: item)
        let frame = rowView.convert(rowView.bounds, to: self)
        draggingItem.setDraggingFrame(frame, contents: permissionHostCachedImage(rowView))
        let session = beginDraggingSession(with: [draggingItem], event: event, source: self)
        owner.recordDragSession(session)
        session.animatesToStartingPositionsOnCancelOrFail = true
    }

    func pasteboard(_ pasteboard: NSPasteboard?, item: NSPasteboardItem, provideDataForType type: NSPasteboard.PasteboardType) {
        item.setString(URL(fileURLWithPath: permissionHostAppPath).absoluteString, forType: type)
    }

    func draggingSession(_ session: NSDraggingSession, sourceOperationMaskFor context: NSDraggingContext) -> NSDragOperation {
        .copy
    }

    func ignoreModifierKeys(for session: NSDraggingSession) -> Bool { true }

    func draggingSession(_ session: NSDraggingSession, willBeginAt screenPoint: NSPoint) {
        owner?.handleDragBegan()
    }

    func draggingSession(_ session: NSDraggingSession, endedAt screenPoint: NSPoint, operation: NSDragOperation) {
        owner?.handleDragEnded(session: session, screenPoint: screenPoint, operation: operation)
    }
}


@MainActor
extension PermissionHostPresenter {
    func handleAllow(permission: String = "accessibility") {
        guard !closed, !returning, !dragging, terminalDragPanel == nil else { return }
        if permissionsConfigured {
            guard permission == "accessibility" || permission == "screen" else { return }
            guard permissionStatuses[permission] != "granted" else { return }
            guard permission != "screen" || !screenDropReceiptSent else { return }
            guard state == "pending" else { return }
            activePermission = permission
            cardView = initialView?.permissionCardView(forPermission: permission) ?? cardView
            if !requestedPermissions.contains(permission) {
                sourceEndpoint = captureInitialEndpoint(permission: permission)
                requestedPermissions.insert(permission)
                retryReadyPermissions.remove(permission)
                state = "requesting"
                initialView?.setPermissionInteractionsEnabled(false)
                onEvent(permission == "screen" ? "allow-screen" : "allow")
                return
            }
            guard retryReadyPermissions.remove(permission) != nil else { return }
            guard let endpoint = captureInitialEndpoint(permission: permission) else {
                sourceEndpoint = nil
                restoreInitialPage()
                return
            }
            sourceEndpoint = endpoint
            setPermissionState("repairing", permission: permission, message: nil)
            onEvent(permission == "screen" ? "retry-screen" : "retry")
            return
        }
        if state == "pending" && !settled {
            sourceEndpoint = captureInitialEndpoint()
            settled = true
            onEvent("allow")
            return
        }
        guard state == "pending", settled, retryReady else { return }
        retryReady = false
        guard let endpoint = captureInitialEndpoint() else {
            sourceEndpoint = nil
            restoreInitialPage()
            return
        }
        sourceEndpoint = endpoint
        setState("repairing", message: nil)
        onEvent("retry")
    }

    func handleSkip() {
        guard !closed, helperPanel == nil else { return }
        onEvent("later")
        close()
    }

    func handleResumeSettings(permission: String = "accessibility") {
        guard !closed, !returning, !dragging, state == "awaiting-user" else { return }
        guard !permissionsConfigured || activePermission == permission else { return }
        if permissionsConfigured {
            setPermissionState("repairing", permission: permission)
        }
        onEvent(permissionsConfigured && permission == "screen" ? "retry-screen" : "retry")
    }

    func handleLater() {
        guard !closed else { return }
        if state == "awaiting-user", helperPanel != nil {
            startBackFlight()
        } else {
            onEvent("later")
            close()
        }
    }

    func handleArrowEntered() {
        guard presented, !closed else { return }
        stretchArrow()
    }

    func handleDragBegan() {
        guard !closed else { return }
        dragging = true
        stopArrow()
        if NSWorkspace.shared.accessibilityDisplayShouldReduceMotion {
            arrowView?.resetToIdentity()
        } else {
            arrowView?.animate(toScaleX: 1, scaleY: 1)
        }
        appRowView?.isHidden = true
    }

    func handleDragEnded() {
        dragging = false
        dragSession = nil
        if closed || terminalDragPanel != nil { finishTerminalDrag(); return }
        appRowView?.isHidden = false
        scheduleArrow(after: 4)
    }

    func handleDragEnded(
        session: NSDraggingSession,
        screenPoint: NSPoint,
        operation: NSDragOperation,
    ) {
        if let terminalDragSession, terminalDragSession === session {
            handleDragEnded()
            return
        }
        guard let dragSession, dragSession === session else { return }
        if receivesManualScreenDrop(at: screenPoint, operation: operation, session: session) {
            screenDropReceiptSent = true
            onEvent("settings-window-drop-received")
        }
        handleDragEnded()
    }

    private func receivesManualScreenDrop(
        at screenPoint: NSPoint,
        operation: NSDragOperation,
        session: NSDraggingSession,
    ) -> Bool {
        guard !closed, terminalDragPanel == nil,
              permissionsConfigured, screenVerification == "manual",
              !screenDropReceiptSent, dragging, dragSession === session,
              state == "awaiting-user", activePermission == "screen",
              permissionStatuses["screen"] == "unknown",
              requestedPermissions.contains("screen"), helperPanel != nil,
              operation == .copy,
              let point = permissionHostQuartzPoint(fromAppKit: screenPoint),
              let settings = settingsWindowLocator(),
              settings.pid > 0, settings.windowID != 0,
              settings.frame.origin.x.isFinite, settings.frame.origin.y.isFinite,
              settings.frame.width.isFinite, settings.frame.width > 0,
              settings.frame.height.isFinite, settings.frame.height > 0,
              settings.frame.contains(point), settingsWindowIsFrontmost(settings) else { return false }
        return true
    }

    func recordDragSession(_ session: NSDraggingSession) {
        if closed || terminalDragPanel != nil {
            terminalDragSession = session
        } else {
            dragSession = session
        }
    }

    func finishTerminalDrag() {
        guard let panel = terminalDragPanel else { return }
        terminalDragPanel = nil
        terminalDragSession = nil
        panel.delegate = nil
        panel.orderOut(nil)
        panel.close()
    }

    private func report(_ error: Error) { reportMessage(error.localizedDescription) }

    func reportMessage(_ message: String) {
        onError(String(message.prefix(512)))
    }

    @discardableResult
    func setGuideActivationPolicy(_ policy: NSApplication.ActivationPolicy) -> Bool {
        let application = NSApplication.shared
        if application.activationPolicy() == policy { return true }
        guard application.setActivationPolicy(policy) else {
            reportMessage("native permission guide could not change its activation policy")
            return false
        }
        return true
    }

    func handleWindowClosed() {
        guard !closed else { return }
        onEvent("later")
        close()
    }

    @discardableResult
    func fitInitialPage() -> Bool {
        guard let initialView, let initialPanel else { return false }
        let preferred = initialView.preferredContentSize
        guard preferred.width.isFinite, preferred.width > 0, preferred.height > 0, preferred.height.isFinite else {
            reportMessage("native permission initial view has invalid preferred size")
            return false
        }
        initialView.setFrameSize(preferred)
        initialPanel.setContentSize(preferred)
        initialView.layoutSubtreeIfNeeded()
        return true
    }

    func setInitialContent(title: String, body: String, allowEnabled: Bool, settingsPlaceholder: Bool) {
        if permissionsConfigured {
            cardView?.isHidden = settingsPlaceholder
            return
        }
        initialView?.setContent(
            title: title,
            body: body,
            allowEnabled: allowEnabled,
            settingsPlaceholder: settingsPlaceholder,
        )
        cardView?.isHidden = settingsPlaceholder
    }

    func captureInitialEndpoint(permission: String? = nil) -> PermissionHostFlightEndpoint? {
        guard let initialView, let cardView else { return nil }
        let scale = initialView.window?.backingScaleFactor ?? 1
        var image = permission.map { initialView.snapshotPermissionCard(scale: scale, permission: $0) }
            ?? initialView.snapshotPermissionCard(scale: scale)
        if image == nil {
            initialView.displayIfNeeded()
            image = permission.map { initialView.snapshotPermissionCard(scale: scale, permission: $0) }
                ?? initialView.snapshotPermissionCard(scale: scale)
        }
        guard let image else { return nil }
        return PermissionHostFlightEndpoint(
            view: cardView,
            frame: permissionHostScreenFrame(cardView),
            radius: 24,
            image: image,
        )
    }

    func helperEndpoint() -> PermissionHostFlightEndpoint? {
        guard let helperView, let helperPanel else { return nil }
        let frame = permissionHostScreenFrame(helperView)
        let scale = helperPanel.backingScaleFactor
        return PermissionHostFlightEndpoint(
            view: helperView,
            frame: frame,
            radius: 14,
            image: nil,
            captureImage: { [weak helperView] in
                guard let helperView else { return nil }
                return helperView.snapshotImage(scale: scale)
            },
        )
    }

    func startSettingsTracking() {
        guard trackingTimer == nil else { return }
        locatingAttempts = 0
        trackingTimer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                self?.placeHelper()
            }
        }
        placeHelper()
    }

    func stopTracking() {
        trackingTimer?.invalidate()
        trackingTimer = nil
        locatingAttempts = 0
    }

    func placeHelper() {
        guard !closed, state == "awaiting-user", !returning, !dragging else { return }
        // The locator returns the selected Settings window in Quartz global
        // coordinates.  Convert only its frame here; keeping the PID/window
        // identity in the locator prevents a same-bundle or stale window from
        // being treated as the current target.
        guard let target = SettingsLocator().locate() else {
            locatingAttempts += 1
            if helperPanel != nil {
                if locatingAttempts >= 10, !dragging, !permissionsConfigured {
                    onEvent("later")
                    close()
                }
            } else if locatingAttempts >= 50 {
                setState("error", message: permissionHostString(copy, "errorBody"))
            }
            return
        }
        locatingAttempts = 0
        if let helperPanel, let helperView {
            if let frame = permissionHostFrame(target.frame, size: helperView.preferredContentSize) {
                helperPanel.setFrame(frame, display: false)
                positionArrow(from: frame)
            }
            return
        }
        guard let frame = permissionHostFrame(target.frame, size: NSSize(width: permissionHostHelperWidth, height: permissionHostHelperHeight)) else {
            setState("error", message: "no display is available for the permission guide")
            return
        }
        createHelper(frame: frame)
        if let helperPanel, let helperView,
           let fittedFrame = permissionHostFrame(target.frame, size: helperView.preferredContentSize) {
            helperPanel.setFrame(fittedFrame, display: false)
            positionArrow(from: fittedFrame)
            if sourceEndpoint != nil {
                startForwardFlight()
            } else {
                revealHelper()
            }
        }
    }

    func createHelper(frame: NSRect) {
        guard let appIcon = permissionHostAppIcon() else {
            setState("error", message: "ChatGPT icon is unavailable")
            return
        }
        let panel = PermissionHostHelperPanel(
            contentRect: frame,
            styleMask: [.titled, .utilityWindow, .nonactivatingPanel, .fullSizeContentView],
            backing: .buffered,
            defer: false,
        )
        panel.isReleasedWhenClosed = false
        panel.isOpaque = false
        panel.backgroundColor = NSColor.white.withAlphaComponent(0.001)
        panel.hasShadow = true
        panel.ignoresMouseEvents = false
        panel.isMovableByWindowBackground = false
        panel.isMovable = false
        panel.titleVisibility = .hidden
        panel.titlebarAppearsTransparent = true
        if #available(macOS 11.0, *) { panel.toolbarStyle = .unifiedCompact }
        panel.collectionBehavior = NSWindow.CollectionBehavior(rawValue: 0x24a)
        panel.level = NSWindow.Level(rawValue: 3)
        panel.hidesOnDeactivate = false
        let delegate = PermissionHostWindowDelegate { [weak self] in self?.handleWindowClosed() }
        panel.delegate = delegate

        let view = IncodexPermissionHelperView(frame: NSRect(x: 0, y: 0, width: permissionHostHelperWidth, height: permissionHostHelperHeight))
        view.configure(
            copy: activeHelperCopy(),
            appIcon: appIcon,
            actionTarget: actionTarget,
        )
        let preferred = view.preferredContentSize
        guard preferred.width.isFinite, preferred.width > 0, preferred.height > 0, preferred.height.isFinite else {
            panel.delegate = nil
            panel.close()
            setState("error", message: "native permission helper has invalid preferred size")
            return
        }
        view.frame = NSRect(origin: .zero, size: preferred)
        let controller = NSViewController()
        controller.view = view
        panel.contentViewController = controller

        let row = PermissionHostDragView(frame: view.appRowFrame)
        row.owner = self
        row.rowView = view.appRowView
        view.addSubview(row)

        let arrowPanel = PermissionHostArrowPanel(
            contentRect: NSRect(x: 0, y: 0, width: permissionHostArrowWindowSize, height: permissionHostArrowWindowSize),
            styleMask: [],
            backing: .buffered,
            defer: false,
        )
        arrowPanel.isReleasedWhenClosed = false
        arrowPanel.isOpaque = false
        arrowPanel.backgroundColor = .clear
        arrowPanel.hasShadow = false
        arrowPanel.ignoresMouseEvents = false
        arrowPanel.collectionBehavior = NSWindow.CollectionBehavior(rawValue: 4)
        arrowPanel.level = NSWindow.Level(rawValue: 3)
        arrowPanel.hidesOnDeactivate = false
        let arrowDelegate = PermissionHostWindowDelegate { [weak self] in self?.handleWindowClosed() }
        arrowPanel.delegate = arrowDelegate
        let canvas = NSView(frame: NSRect(x: 0, y: 0, width: permissionHostArrowWindowSize, height: permissionHostArrowWindowSize))
        canvas.wantsLayer = true
        canvas.layer?.masksToBounds = false
        let arrow = IncodexPermissionArrowView(frame: NSRect(x: 36, y: 10, width: permissionHostArrowGraphicSize, height: permissionHostArrowGraphicSize))
        let tracker = PermissionHostArrowTrackerView(frame: arrow.frame)
        tracker.owner = self
        canvas.addSubview(arrow)
        canvas.addSubview(tracker)
        arrowPanel.contentView = canvas
        panel.addChildWindow(arrowPanel, ordered: .above)

        self.helperPanel = panel
        self.helperView = view
        self.appRowView = view.appRowView
        self.dragView = row
        self.arrowPanel = arrowPanel
        self.arrowView = arrow
        self.arrowTracker = tracker
        self.helperDelegate = delegate
        self.arrowDelegate = arrowDelegate
        positionArrow(from: frame)
    }

    func positionArrow(from frame: NSRect) {
        guard let arrowPanel else { return }
        let x = layoutDirection == "rightToLeft"
            ? frame.width - permissionHostArrowWindowX - permissionHostArrowWindowSize
            : permissionHostArrowWindowX
        arrowPanel.setFrame(
            NSRect(
                x: frame.minX + x,
                y: frame.minY + permissionHostArrowWindowY,
                width: permissionHostArrowWindowSize,
                height: permissionHostArrowWindowSize,
            ),
            display: false,
        )
    }

    func revealHelper() {
        guard !closed, state == "awaiting-user", !returning else { return }
        presented = true
        helperPanel?.orderFrontRegardless()
        arrowPanel?.orderFront(nil)
        let applications = NSRunningApplication.runningApplications(withBundleIdentifier: SettingsLocator.bundleIdentifier)
        if applications.count == 1, let application = applications.first {
            _ = application.activate(options: .activateAllWindows)
        }
        // A mouse press on the nonactivating helper can still activate this
        // process before the drag view receives mouseDown.  That activation
        // raises the separate initial NSWindow above Settings.  During the
        // guide, keep the existing windows and native drag, but disallow
        // process activation; Back restores the ordinary policy before its
        // existing activation and reverse flight.
        guard setGuideActivationPolicy(.prohibited) else { return }
        scheduleArrow(after: 0.5)
    }

    func activeHelperCopy() -> NSDictionary {
        let result = NSMutableDictionary(dictionary: permissionHostNativeCopy(copy, layoutDirection: layoutDirection))
        guard permissionsConfigured, activePermission == "screen" else { return result }
        for (helperKey, screenKey) in [
            ("addedTitle", "screenAddedTitle"),
            ("addedBody", "screenAddedBody"),
            ("dragInstruction", "screenDragInstruction"),
            ("dragInstructionRuns", "screenDragInstructionRuns"),
            ("checking", "screenChecking"),
        ] {
            result[helperKey] = permissionHostString(copy, screenKey)
        }
        return result
    }

}
