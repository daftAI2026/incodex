import AppKit
import Foundation

// The SwiftUI views in permission-views.swift are the renderer.  This file is
// deliberately limited to the short-lived AppKit presenter: panel lifetime,
// screen/window geometry, drag tracking, and the native guide state machine.
// TCC inspection, reset, and Settings launching remain in the CLI process.

private let permissionHostAppPath = "/Applications/ChatGPT.app"
private let permissionHostInitialWidth: CGFloat = 600
private let permissionHostHelperWidth: CGFloat = 531
private let permissionHostHelperHeight: CGFloat = 110
private let permissionHostRowWidth: CGFloat = 459
private let permissionHostRowHeight: CGFloat = 42
private let permissionHostArrowWindowSize: CGFloat = 100
private let permissionHostArrowWindowX: CGFloat = 30
private let permissionHostArrowWindowY: CGFloat = 60
private let permissionHostArrowGraphicSize: CGFloat = 28

@MainActor
func permissionHostAppIcon() -> NSImage? {
    NSImage(contentsOfFile: "\(permissionHostAppPath)/Contents/Resources/icon-chatgpt.png")
}

@MainActor
func permissionHostPermissionIcon(title: String) -> NSImage? {
    NSImage(contentsOfFile: "/System/Library/ExtensionKit/Extensions/AccessibilitySettingsExtension.appex/Contents/Resources/UniversalAccessPref.icns")
        ?? NSImage(contentsOfFile: "/System/Library/PreferencePanes/UniversalAccessPref.prefPane/Contents/Resources/UniversalAccessPref.icns")
        ?? NSImage(systemSymbolName: "accessibility", accessibilityDescription: title)
}

private func permissionHostString(_ copy: NSDictionary, _ key: String) -> String {
    if let value = copy[key] as? String { return value }
    if let value = copy[key] as? NSString { return value as String }
    return ""
}

private func permissionHostNativeCopy(_ copy: NSDictionary, layoutDirection: String) -> NSDictionary {
    let keys = [
        "title", "body", "permissionTitle", "permissionDescription", "repair", "later", "back",
        "addedTitle", "addedBody", "dragInstruction", "dragInstructionRuns", "completeInSettings",
        "checking", "repairing", "openSettings", "errorTitle", "errorBody",
    ]
    let result = NSMutableDictionary(capacity: keys.count + 1)
    for key in keys { result[key] = permissionHostString(copy, key) }
    result["layoutDirection"] = layoutDirection == "rightToLeft" ? "rightToLeft" : "leftToRight"
    return result
}

private func permissionHostCachedImage(_ view: NSView) -> NSImage? {
    view.layoutSubtreeIfNeeded()
    let bounds = view.bounds
    guard bounds.width > 0, bounds.height > 0,
          let representation = view.bitmapImageRepForCachingDisplay(in: bounds) else { return nil }
    view.displayIfNeeded()
    view.cacheDisplay(in: bounds, to: representation)
    let image = NSImage(size: bounds.size)
    image.addRepresentation(representation)
    return image
}

private func permissionHostScreenFrame(_ view: NSView) -> NSRect {
    guard let window = view.window else { return view.bounds }
    let windowRect = view.convert(view.bounds, to: nil)
    return window.convertToScreen(windowRect)
}

private func permissionHostIntegral(_ value: CGFloat) -> CGFloat { floor(value) }

private func permissionHostFrame(_ target: NSRect, size: NSSize) -> NSRect? {
    guard let screen = NSScreen.screens.first else { return nil }
    let x = target.minX + target.width - size.width - 10
    let y = screen.frame.minY + screen.frame.height - target.minY - target.height + 10
    let left = permissionHostIntegral(x)
    let bottom = permissionHostIntegral(y)
    return NSRect(
        x: left,
        y: bottom,
        width: ceil(x + size.width) - left,
        height: ceil(y + size.height) - bottom,
    )
}

@MainActor
private final class PermissionHostWindowDelegate: NSObject, NSWindowDelegate {
    let onClose: () -> Void

    init(onClose: @escaping () -> Void) {
        self.onClose = onClose
        super.init()
    }

    func windowWillClose(_ notification: Notification) { onClose() }
}

@MainActor
private final class PermissionHostActionTarget: NSObject {
    weak var owner: PermissionHostPresenter?

    init(owner: PermissionHostPresenter) {
        self.owner = owner
        super.init()
    }

    @objc(allow:)
    func allow(_ sender: Any?) { owner?.handleAllow() }

    @objc(skip:)
    func skip(_ sender: Any?) { owner?.handleSkip() }

    @objc(resumeSettings:)
    func resumeSettings(_ sender: Any?) { owner?.handleResumeSettings() }

    @objc(later:)
    func later(_ sender: Any?) { owner?.handleLater() }
}

@MainActor
private final class PermissionHostHelperPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

@MainActor
private final class PermissionHostArrowPanel: NSPanel {
    override var canBecomeKey: Bool { false }
    override var canBecomeMain: Bool { false }
}

@MainActor
private final class PermissionHostArrowTrackerView: NSView {
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
private final class PermissionHostDragView: NSView, NSDraggingSource, NSPasteboardItemDataProvider {
    weak var owner: PermissionHostPresenter?
    weak var rowView: NSView?

    override var isFlipped: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func shouldDelayWindowOrdering(for event: NSEvent) -> Bool {
        owner?.canStartDrag ?? false
    }

    override func mouseDown(with event: NSEvent) {
        guard let owner, owner.canStartDrag, let rowView else { return }
        let item = NSPasteboardItem()
        item.setDataProvider(self, forTypes: [.fileURL])
        let draggingItem = NSDraggingItem(pasteboardWriter: item)
        let frame = rowView.convert(rowView.bounds, to: self)
        draggingItem.setDraggingFrame(frame, contents: permissionHostCachedImage(rowView))
        let session = beginDraggingSession(with: [draggingItem], event: event, source: self)
        // Complete the mouse-down ordering delay after starting the native
        // drag. Otherwise AppKit may activate the host and lift its initial
        // window over System Settings while the pointer is still held down.
        NSApplication.shared.preventWindowOrdering()
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
        owner?.handleDragEnded()
    }
}

@MainActor
public final class PermissionHostPresenter: NSObject {
    private let copy: NSDictionary
    private let layoutDirection: String
    private let onEvent: (String) -> Void
    private let onError: (String) -> Void
    private lazy var actionTarget = PermissionHostActionTarget(owner: self)

    private var initialPanel: NSWindow?
    private var initialView: IncodexPermissionInitialView?
    private var cardView: NSView?
    private var helperPanel: NSPanel?
    private var helperView: IncodexPermissionHelperView?
    private var appRowView: NSView?
    private var dragView: PermissionHostDragView?
    private var dragSession: NSDraggingSession?
    private var terminalDragPanel: NSPanel?
    private var terminalDragSession: NSDraggingSession?
    private var arrowPanel: NSPanel?
    private var arrowView: IncodexPermissionArrowView?
    private var arrowTracker: PermissionHostArrowTrackerView?
    private var initialDelegate: PermissionHostWindowDelegate?
    private var helperDelegate: PermissionHostWindowDelegate?
    private var arrowDelegate: PermissionHostWindowDelegate?
    private var trackingTimer: Timer?
    private var arrowTimer: Timer?
    private var arrowReturnTimer: Timer?
    private var backFlightTimer: Timer?
    private var flight: PermissionHostFlight?
    private var sourceEndpoint: PermissionHostFlightEndpoint?
    private var state = "pending"
    private var closed = false
    private var presented = false
    private var settled = false
    private var retryReady = false
    private var returning = false
    private var dragging = false
    private var locatingAttempts = 0
    private var forwardSequence = 0
    private var returnSequence = 0

    public init(
        copy: NSDictionary,
        layoutDirection: String,
        onEvent: @escaping (String) -> Void,
        onError: @escaping (String) -> Void,
    ) {
        self.copy = copy.copy() as? NSDictionary ?? copy
        self.layoutDirection = layoutDirection == "rightToLeft" ? "rightToLeft" : "leftToRight"
        self.onEvent = onEvent
        self.onError = onError
        super.init()
    }

    public var canStartDrag: Bool {
        !closed && !dragging && state == "awaiting-user" && helperPanel != nil
    }

    @discardableResult
    public func present() -> Bool {
        guard !closed, !presented else { return !closed }
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
            panel.title = ""
            panel.titleVisibility = .hidden
            panel.titlebarAppearsTransparent = true
            panel.isMovableByWindowBackground = true
            panel.isOpaque = false
            panel.backgroundColor = NSColor.white.withAlphaComponent(0.001)
            let delegate = PermissionHostWindowDelegate { [weak self] in self?.handleWindowClosed() }
            panel.delegate = delegate

            let view = IncodexPermissionInitialView(frame: panel.contentView?.bounds ?? NSRect(x: 0, y: 0, width: permissionHostInitialWidth, height: 312))
            let card = view.permissionCardView
            let permissionIcon = permissionHostPermissionIcon(title: permissionHostString(self.copy, "permissionTitle"))
            view.configure(
                copy: permissionHostNativeCopy(self.copy, layoutDirection: layoutDirection),
                appIcon: appIcon,
                permissionIcon: permissionIcon,
                actionTarget: actionTarget,
            )
            view.setContent(
                title: permissionHostString(self.copy, "title"),
                body: permissionHostString(self.copy, "body"),
                allowEnabled: true,
                settingsPlaceholder: false,
            )
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
        if (dragging || dragSession != nil), let helperPanel {
            terminalDragPanel = helperPanel
            terminalDragSession = dragSession
            terminalDragPanel?.alphaValue = 0
        }
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
    }

    fileprivate func handleAllow() {
        guard !closed, !returning, !dragging else { return }
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

    fileprivate func handleSkip() {
        guard !closed, helperPanel == nil else { return }
        onEvent("later")
        close()
    }

    fileprivate func handleResumeSettings() {
        guard !closed, !returning, !dragging, state == "awaiting-user" else { return }
        onEvent("retry")
    }

    fileprivate func handleLater() {
        guard !closed else { return }
        if state == "awaiting-user", helperPanel != nil {
            startBackFlight()
        } else {
            onEvent("later")
            close()
        }
    }

    fileprivate func handleArrowEntered() {
        guard presented, !closed else { return }
        stretchArrow()
    }

    fileprivate func handleDragBegan() {
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

    fileprivate func handleDragEnded() {
        dragging = false
        dragSession = nil
        if closed { finishTerminalDrag(); return }
        appRowView?.isHidden = false
        scheduleArrow(after: 4)
    }

    fileprivate func recordDragSession(_ session: NSDraggingSession) {
        if closed {
            terminalDragSession = session
        } else {
            dragSession = session
        }
    }

    private func finishTerminalDrag() {
        guard let panel = terminalDragPanel else { return }
        terminalDragPanel = nil
        terminalDragSession = nil
        panel.delegate = nil
        panel.orderOut(nil)
        panel.close()
    }

    private func report(_ error: Error) { reportMessage(error.localizedDescription) }

    private func reportMessage(_ message: String) {
        onError(String(message.prefix(512)))
    }

    private func handleWindowClosed() {
        guard !closed else { return }
        onEvent("later")
        close()
    }

    @discardableResult
    private func fitInitialPage() -> Bool {
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

    private func setInitialContent(title: String, body: String, allowEnabled: Bool, settingsPlaceholder: Bool) {
        initialView?.setContent(
            title: title,
            body: body,
            allowEnabled: allowEnabled,
            settingsPlaceholder: settingsPlaceholder,
        )
        cardView?.isHidden = settingsPlaceholder
    }

    private func captureInitialEndpoint() -> PermissionHostFlightEndpoint? {
        guard let initialView, let cardView else { return nil }
        let scale = initialView.window?.backingScaleFactor ?? 1
        var image = initialView.snapshotPermissionCard(scale: scale)
        if image == nil {
            initialView.displayIfNeeded()
            image = initialView.snapshotPermissionCard(scale: scale)
        }
        guard let image else { return nil }
        return PermissionHostFlightEndpoint(
            view: cardView,
            frame: permissionHostScreenFrame(cardView),
            radius: 24,
            image: image,
        )
    }

    private func helperEndpoint() -> PermissionHostFlightEndpoint? {
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

    private func startSettingsTracking() {
        guard trackingTimer == nil else { return }
        locatingAttempts = 0
        trackingTimer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                self?.placeHelper()
            }
        }
        placeHelper()
    }

    private func stopTracking() {
        trackingTimer?.invalidate()
        trackingTimer = nil
        locatingAttempts = 0
    }

    private func placeHelper() {
        guard !closed, state == "awaiting-user", !returning, !dragging else { return }
        // The locator returns the selected Settings window in Quartz global
        // coordinates.  Convert only its frame here; keeping the PID/window
        // identity in the locator prevents a same-bundle or stale window from
        // being treated as the current target.
        guard let target = SettingsLocator().locate() else {
            locatingAttempts += 1
            if helperPanel != nil {
                if locatingAttempts >= 10, !dragging {
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

    private func createHelper(frame: NSRect) {
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
            copy: permissionHostNativeCopy(copy, layoutDirection: layoutDirection),
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

    private func positionArrow(from frame: NSRect) {
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

    private func revealHelper() {
        guard !closed, state == "awaiting-user", !returning else { return }
        presented = true
        helperPanel?.orderFrontRegardless()
        arrowPanel?.orderFront(nil)
        let applications = NSRunningApplication.runningApplications(withBundleIdentifier: SettingsLocator.bundleIdentifier)
        if applications.count == 1, let application = applications.first {
            _ = application.activate(options: .activateAllWindows)
        }
        scheduleArrow(after: 0.5)
    }

    private func startForwardFlight() {
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

    private func startBackFlight() {
        guard !closed, !returning, !dragging, state == "awaiting-user",
              let initialView,
              let cardView else { return }
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

    private func restoreInitialPage(raiseBeforeHelperDisposal: Bool = false) {
        guard !closed else { return }
        if raiseBeforeHelperDisposal { initialPanel?.level = .floating }
        disposeHelper()
        returning = false
        state = "pending"
        retryReady = true
        setInitialContent(
            title: permissionHostString(copy, "title"),
            body: permissionHostString(copy, "body"),
            allowEnabled: true,
            settingsPlaceholder: false,
        )
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
        stopArrow()
        dragView?.removeFromSuperview()
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
            helperPanel.close()
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
        dragging = false
    }

    private func stopBackFlightTimer() {
        backFlightTimer?.invalidate()
        backFlightTimer = nil
    }

    private func scheduleArrow(after delay: TimeInterval) {
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

    private func stretchArrow() {
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

    private func stopArrow() {
        arrowTimer?.invalidate()
        arrowTimer = nil
        arrowReturnTimer?.invalidate()
        arrowReturnTimer = nil
    }
}
