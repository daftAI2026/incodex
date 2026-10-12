/**
 * [INPUT]: 依赖双权限视图、presenter 与 AppKit 只读系统图标。
 * [OUTPUT]: 验证双行、系统录屏图标与飞行/拖拽生命周期；夹具窗口不显示。
 * [POS]: 双权限卡隔离契约；不访问宿主应用、系统设置或 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import AppKit
import Foundation
import SwiftUI
import UniformTypeIdentifiers

@MainActor
struct PermissionHostFlightEndpoint {
    let view: NSView
    let frame: NSRect
    let radius: CGFloat
    let image: NSImage?
    let captureImage: (() -> NSImage?)?

    init(view: NSView, frame: NSRect, radius: CGFloat, image: NSImage? = nil, captureImage: (() -> NSImage?)? = nil) {
        self.view = view
        self.frame = frame
        self.radius = radius
        self.image = image
        self.captureImage = captureImage
    }
}

@MainActor
final class PermissionHostFlight {
    static var latest: PermissionHostFlight?
    let reverse: Bool
    private let onComplete: () -> Void
    var started = false
    var disposed = false
    var disposeCount = 0

    init(source: PermissionHostFlightEndpoint, target: @escaping () -> PermissionHostFlightEndpoint, reverse: Bool = false, isClosed: @escaping () -> Bool, onComplete: @escaping () -> Void, onError: @escaping (String) -> Void) {
        self.reverse = reverse
        self.onComplete = onComplete
        Self.latest = self
    }

    func start() { started = true }

    func dispose() {
        guard !disposed else { return }
        disposed = true
        disposeCount += 1
        // Match the product flight's synchronous completion callback.
        onComplete()
    }
}

@MainActor
private final class PermissionDragFixturePanel: NSPanel {
    var closeCount = 0

    override func close() {
        closeCount += 1
        super.close()
    }
}

@MainActor
struct PermissionHostSettingsFrame {
    let frame: NSRect
    let pid: pid_t
    let windowID: CGWindowID
}

@MainActor
final class SettingsLocator {
    static let bundleIdentifier = "com.apple.systempreferences"
    func locate() -> PermissionHostSettingsFrame? { nil }
    func prepareHandoff() {}
}

@MainActor
private final class DualPermissionActionTarget: NSObject {
    var accessibilityAllows = 0
    var screenAllows = 0
    var accessibilityResumes = 0
    var screenResumes = 0

    @objc(allow:)
    func allow(_ sender: Any?) { accessibilityAllows += 1 }
    @objc(allowScreen:)
    func allowScreen(_ sender: Any?) { screenAllows += 1 }
    @objc(resumeSettings:)
    func resumeSettings(_ sender: Any?) { accessibilityResumes += 1 }
    @objc(resumeScreenSettings:)
    func resumeScreenSettings(_ sender: Any?) { screenResumes += 1 }
}

@MainActor
private func permissionRows(in value: Any, depth: Int = 0) -> [PermissionRowState] {
    if let row = value as? PermissionRowState { return [row] }
    if let rows = value as? [PermissionRowState] { return rows }
    if let rows = value as? [String: PermissionRowState] { return Array(rows.values) }
    guard depth < 5 else { return [] }
    let mirror = Mirror(reflecting: value)
    let typeName = String(describing: mirror.subjectType)
    guard typeName.contains("PermissionInitialState") else { return [] }
    return mirror.children.flatMap { permissionRows(in: $0.value, depth: depth + 1) }
}

@MainActor
private func privateBool(_ value: Any, named label: String) -> Bool {
    guard let child = Mirror(reflecting: value).children.first(where: { $0.label == label }),
          let result = child.value as? Bool else {
        fatalError("missing reflected Boolean \(label)")
    }
    return result
}

@MainActor
private func privateValue(_ value: Any, named label: String) -> Any? {
    guard let child = Mirror(reflecting: value).children.first(where: { $0.label == label }) else {
        fatalError("missing reflected value \(label)")
    }
    var result = child.value
    while Mirror(reflecting: result).displayStyle == .optional {
        guard let wrapped = Mirror(reflecting: result).children.first else { return nil }
        result = wrapped.value
    }
    return result
}

@MainActor
private func flightFixturePresenter(
    copy: NSDictionary,
    activePermission: String,
    needsInitialView: Bool,
    tracksDragSource: Bool = false,
    onEvent: @escaping (String) -> Void = { _ in },
) -> PermissionHostPresenter {
    let presenter = PermissionHostPresenter(
        copy: copy,
        layoutDirection: "leftToRight",
        onEvent: onEvent,
        onError: { _ in },
    )
    presenter.configurePermissions(["accessibility": "not-granted", "screen": "not-granted"])
    presenter.presented = true
    presenter.activePermission = activePermission
    presenter.state = "awaiting-user"

    if needsInitialView {
        let initialView = IncodexPermissionInitialView(frame: NSRect(x: 0, y: 0, width: 600, height: 400))
        initialView.configurePermissions(
            copy: copy,
            appIcon: NSImage(size: NSSize(width: 64, height: 64)),
            accessibilityIcon: NSImage(size: NSSize(width: 64, height: 64)),
            screenIcon: NSImage(size: NSSize(width: 64, height: 64)),
            actionTarget: NSObject(),
        )
        presenter.initialView = initialView
        presenter.cardView = initialView.permissionCardView(forPermission: activePermission)
    }

    let helperFrame = NSRect(x: -10_000, y: -10_000, width: 531, height: 110)
    let helperPanel: NSPanel
    if tracksDragSource {
        helperPanel = PermissionDragFixturePanel(
            contentRect: helperFrame,
            styleMask: [.borderless],
            backing: .buffered,
            defer: false,
        )
    } else {
        helperPanel = PermissionHostHelperPanel(
            contentRect: helperFrame,
            styleMask: [.borderless],
            backing: .buffered,
            defer: false,
        )
    }
    let helperView = IncodexPermissionHelperView(frame: NSRect(x: 0, y: 0, width: 531, height: 110))
    helperPanel.contentView = helperView
    presenter.helperPanel = helperPanel
    presenter.helperView = helperView
    return presenter
}

@MainActor
@main
struct PermissionDualRowsSmoke {
    static func main() {
        _ = NSApplication.shared
        // -- 系统图标：已登记时必须复用系统设置的彩色资产 --
        if let screenType = UTType("com.apple.graphic-icon.screen-recording") {
            let expectedIcon = NSWorkspace.shared.icon(for: screenType)
            guard let screenIcon = permissionHostScreenIcon(title: "Screen & System Audio Recording") else {
                fatalError("screen permission must provide an icon")
            }
            precondition(!screenIcon.isTemplate,
                         "screen permission must use the colored system icon, not a template SF Symbol")
            precondition(screenIcon.tiffRepresentation == expectedIcon.tiffRepresentation,
                         "screen permission must reuse the registered macOS recording icon")
            print("system screen recording icon matches registered macOS icon")
        }
        let copy: NSDictionary = [
            "title": "Update ChatGPT permissions",
            "body": "Confirm the permissions used by the installed ChatGPT app.",
            "permissionTitle": "Accessibility",
            "permissionDescription": "Read and control app interfaces",
            "repair": "Allow",
            "later": "Back",
            "completeInSettings": "Complete in Settings",
            "screenTitle": "Screen Recording",
            "screenDescription": "Allow ChatGPT to record screen content.",
            "screenAddedTitle": "Add ChatGPT",
            "screenAddedBody": "Select ChatGPT in Screen Recording settings.",
            "screenDragInstruction": "Drag ChatGPT into the app list above.",
            "screenDragInstructionRuns": "Drag ChatGPT into the app list above.",
            "screenChecking": "Checking Screen Recording permission…",
            "done": "Done",
            "screenUnknown": "Not confirmed",
            "back": "Back",
        ]
        let target = DualPermissionActionTarget()
        let appIcon = NSImage(size: NSSize(width: 64, height: 64))
        let accessibilityIcon = NSImage(size: NSSize(width: 64, height: 64))
        let screenIcon = NSImage(size: NSSize(width: 64, height: 64))

        let single = IncodexPermissionInitialView(frame: .zero)
        single.configure(
            copy: copy,
            appIcon: appIcon,
            permissionIcon: accessibilityIcon,
            actionTarget: target,
        )
        single.setContent(
            title: "Update ChatGPT permissions",
            body: "Confirm the permissions used by the installed ChatGPT app.",
            allowEnabled: true,
            settingsPlaceholder: false,
        )

        let dual = IncodexPermissionInitialView(frame: .zero)
        dual.configurePermissions(
            copy: copy,
            appIcon: appIcon,
            accessibilityIcon: accessibilityIcon,
            screenIcon: screenIcon,
            actionTarget: target,
        )

        let accessibilityCard = dual.permissionCardView(forPermission: "accessibility")
        let screenCard = dual.permissionCardView(forPermission: "screen")
        guard let accessibilityCard, let screenCard else {
            fatalError("dual permission configuration must create both independent row hosts")
        }
        precondition(accessibilityCard !== screenCard, "permission rows must not share an embedded view")
        precondition(accessibilityCard.fittingSize == NSSize(width: 518, height: 80))
        precondition(screenCard.fittingSize == NSSize(width: 518, height: 80))
        precondition(abs(dual.preferredContentSize.height - single.preferredContentSize.height - 98) < 0.01,
                     "the second 80pt row and 18pt gap must add exactly 98pt")

        let dualRows = permissionRows(in: Mirror(reflecting: dual).children.first(where: { $0.label == "state" })!.value)
        precondition(dualRows.count == 2, "the dual page must own two row models")
        guard let accessibilityRow = dualRows.first(where: { $0.permissionTitle == "Accessibility" }),
              let screenRow = dualRows.first(where: { $0.permissionTitle == "Screen Recording" }) else {
            fatalError("dual row copy must remain attached to its own row state")
        }
        precondition(accessibilityRow.permissionStatus == "unknown" && screenRow.permissionStatus == "unknown",
                     "unverified TCC state must stay unknown")
        precondition(accessibilityRow.isDefaultAction && !screenRow.isDefaultAction,
                     "the first incomplete row owns the only default Return action")
        screenRow.setActionFocused(true)
        precondition(!accessibilityRow.isDefaultAction && screenRow.isDefaultAction,
                     "keyboard focus moves the default Return action to that pending row")
        screenRow.setActionFocused(false)
        precondition(accessibilityRow.isDefaultAction && !screenRow.isDefaultAction,
                     "clearing focused row restores the first incomplete default action")
        accessibilityRow.performAllow()
        screenRow.performAllow()
        accessibilityRow.resumeSettings()
        screenRow.resumeSettings()
        precondition(target.accessibilityAllows == 1 && target.screenAllows == 1,
                     "each Allow action must reach its own selector")
        precondition(target.accessibilityResumes == 1 && target.screenResumes == 1,
                     "each Settings placeholder must resume its own permission flow")
        accessibilityRow.setContent(allowEnabled: false, settingsPlaceholder: true)
        screenRow.setContent(allowEnabled: true, settingsPlaceholder: false)
        precondition(!accessibilityRow.allowEnabled && accessibilityRow.settingsPlaceholder)
        precondition(screenRow.allowEnabled && !screenRow.settingsPlaceholder)
        dual.setPermissionState("granted", permission: "accessibility")
        dual.setPermissionState("not-granted", permission: "screen")
        let mixedRows = permissionRows(in: Mirror(reflecting: dual).children.first(where: { $0.label == "state" })!.value)
        precondition(mixedRows.count == 2, "a completed row must remain visible beside a pending row")
        let mixedAccessibility = mixedRows.first(where: { $0.permissionTitle == "Accessibility" })!
        let mixedScreen = mixedRows.first(where: { $0.permissionTitle == "Screen Recording" })!
        precondition(mixedAccessibility.permissionStatus == "granted" && mixedScreen.permissionStatus == "not-granted",
                     "one permission update must not overwrite the other row")
        precondition(mixedAccessibility.statusMessage == "Read and control app interfaces"
                     && mixedAccessibility.completionLabel == "Done"
                     && mixedScreen.statusMessage == "Allow ChatGPT to record screen content.",
                     "a granted row keeps its description and shows the ordinary Done label")
        precondition(!mixedAccessibility.allowEnabled && mixedScreen.allowEnabled,
                     "the Done row must not offer Allow while the pending row stays actionable")
        precondition(!mixedAccessibility.isDefaultAction && mixedScreen.isDefaultAction,
                     "Return must skip Done and target the remaining pending row")
        let screenAllowsBeforeDoneAction = target.screenAllows
        mixedAccessibility.performAllow()
        precondition(target.accessibilityAllows == 1 && target.screenAllows == screenAllowsBeforeDoneAction,
                     "Done rows must not dispatch another permission request")
        mixedScreen.performAllow()
        precondition(target.screenAllows == screenAllowsBeforeDoneAction + 1,
                     "pending screen permission remains actionable after Accessibility is done")

        dual.setPermissionState("repairing", permission: "accessibility")
        let busyRows = permissionRows(in: Mirror(reflecting: dual).children.first(where: { $0.label == "state" })!.value)
        precondition(busyRows.allSatisfy { !$0.allowEnabled && !$0.isDefaultAction },
                     "a permission transaction disables both Allow controls and default Return")
        let actionsBeforeBusy = (target.accessibilityAllows, target.screenAllows)
        busyRows.forEach { $0.performAllow() }
        precondition((target.accessibilityAllows, target.screenAllows) == actionsBeforeBusy,
                     "busy rows cannot dispatch a second permission request")
        dual.setPermissionInteractionsEnabled(true)
        dual.setPermissionState("not-granted", permission: "accessibility")
        dual.setPermissionState("not-granted", permission: "screen")

        dual.setPermissionState("not-granted", permission: "accessibility")
        let revokedRows = permissionRows(in: Mirror(reflecting: dual).children.first(where: { $0.label == "state" })!.value)
        precondition(revokedRows.count == 2, "a revoked grant must not remove either permission row")
        let revokedAccessibility = revokedRows.first(where: { $0.permissionTitle == "Accessibility" })!
        let revokedScreen = revokedRows.first(where: { $0.permissionTitle == "Screen Recording" })!
        precondition(revokedAccessibility.permissionStatus == "not-granted"
                     && revokedScreen.permissionStatus == "not-granted",
                     "a fresh mixed snapshot must replace stale grants in both rows")
        precondition(revokedAccessibility.allowEnabled && revokedScreen.allowEnabled,
                     "revoked rows must return to their independent Allow actions")
        guard let revokedSnapshot = dual.snapshotPermissionCard(scale: 1, permission: "accessibility") else {
            fatalError("a revoked row must remain independently snapshotable")
        }
        precondition(revokedSnapshot.size == NSSize(width: 518, height: 80))

        precondition(permissionHostDefaultAction(
            statuses: ["accessibility": "not-granted", "screen": "not-granted"],
            focusedPermission: "screen",
        ) == "screen", "Return must prefer the focused pending permission")
        precondition(permissionHostDefaultAction(
            statuses: ["accessibility": "not-granted", "screen": "not-granted"],
            focusedPermission: nil,
        ) == "accessibility", "Return must fall back to the first pending permission")
        precondition(permissionHostDefaultAction(
            statuses: ["accessibility": "granted", "screen": "not-granted"],
            focusedPermission: "accessibility",
        ) == "screen", "Return must not target a granted permission")
        precondition(permissionHostDefaultAction(
            statuses: ["accessibility": "granted", "screen": "granted"],
            focusedPermission: "screen",
        ) == nil, "the card must expose no Return action after both grants")

        guard let accessibilitySnapshot = dual.snapshotPermissionCard(scale: 1, permission: "accessibility"),
              let screenSnapshot = dual.snapshotPermissionCard(scale: 1, permission: "screen") else {
            fatalError("each permission row must produce an independent foreground snapshot")
        }
        precondition(accessibilitySnapshot.size == NSSize(width: 518, height: 80))
        precondition(screenSnapshot.size == NSSize(width: 518, height: 80))

        var events: [String] = []
        let presenter = PermissionHostPresenter(
            copy: copy,
            layoutDirection: "leftToRight",
            onEvent: { events.append($0) },
            onError: { fatalError($0) },
        )
        presenter.configurePermissions(["accessibility": "not-granted", "screen": "not-granted"])
        presenter.updatePermissions(["accessibility": "granted", "screen": "not-granted"])
        precondition(!privateBool(presenter, named: "closed"),
                     "one permission in a full status snapshot must not close the card")
        presenter.updatePermissions(["accessibility": "not-granted", "screen": "granted"])
        precondition(!privateBool(presenter, named: "closed"),
                     "one permission grant must not combine with a stale cached grant")
        presenter.updatePermissions(["accessibility": "not-granted", "screen": "not-granted"])
        presenter.setPermissionState("granted", permission: "accessibility", message: nil)
        precondition(!privateBool(presenter, named: "closed"), "one granted row must not close the whole card")
        presenter.setPermissionState("granted", permission: "screen", message: nil)
        precondition(!privateBool(presenter, named: "closed"),
                     "row state messages must never decide aggregate completion")
        presenter.updatePermissions(["accessibility": "granted", "screen": "granted"])
        precondition(!privateBool(presenter, named: "closed"),
                     "even a complete cached snapshot cannot decide global completion")
        presenter.setState("granted")
        precondition(privateBool(presenter, named: "closed"),
                     "only the CLI-owned global completion message closes the card")
        precondition(events.isEmpty, "status reconciliation must not emit Allow or retry actions")

        var requestEvents: [String] = []
        let requestPresenter = PermissionHostPresenter(
            copy: copy,
            layoutDirection: "leftToRight",
            onEvent: { requestEvents.append($0) },
            onError: { fatalError($0) },
        )
        requestPresenter.configurePermissions(["accessibility": "not-granted", "screen": "not-granted"])
        requestPresenter.handleAllow(permission: "accessibility")
        requestPresenter.handleAllow(permission: "accessibility")
        requestPresenter.handleAllow(permission: "screen")
        precondition(requestEvents == ["allow"],
                     "one in-flight Allow owns the transaction and rejects duplicate row actions")
        requestPresenter.setPermissionState("granted", permission: "accessibility")
        requestPresenter.handleAllow(permission: "screen")
        requestPresenter.handleAllow(permission: "screen")
        precondition(requestEvents == ["allow", "allow-screen"],
                     "the next pending row sends one distinct screen event after the first grant")

        var revokeEvents: [String] = []
        let revokePresenter = PermissionHostPresenter(
            copy: copy,
            layoutDirection: "leftToRight",
            onEvent: { revokeEvents.append($0) },
            onError: { fatalError($0) },
        )
        revokePresenter.configurePermissions(["accessibility": "not-granted", "screen": "not-granted"])
        revokePresenter.handleAllow(permission: "accessibility")
        revokePresenter.setPermissionState("granted", permission: "accessibility")
        revokePresenter.updatePermissions(["accessibility": "not-granted", "screen": "not-granted"])
        revokePresenter.handleAllow(permission: "accessibility")
        precondition(revokeEvents == ["allow", "allow"],
                     "a full snapshot that revokes an old grant must restore the row's initial Allow action")
        let forwardPresenter = flightFixturePresenter(
            copy: copy,
            activePermission: "accessibility",
            needsInitialView: false,
        )
        forwardPresenter.sourceEndpoint = PermissionHostFlightEndpoint(
            view: NSView(frame: NSRect(x: 0, y: 0, width: 518, height: 80)),
            frame: NSRect(x: 0, y: 0, width: 518, height: 80),
            radius: 24,
            image: NSImage(size: NSSize(width: 518, height: 80)),
        )
        forwardPresenter.trackingTimer = Timer(timeInterval: 60, repeats: true) { _ in }
        forwardPresenter.startForwardFlight()
        guard let forwardFlight = PermissionHostFlight.latest else {
            fatalError("forward transition must retain its active flight")
        }
        precondition(forwardFlight.started && !forwardFlight.disposed)
        precondition(privateValue(forwardPresenter, named: "trackingTimer") is Timer,
                     "the fixture must prove Settings tracking is active before a grant")
        forwardPresenter.updatePermissions(["accessibility": "granted", "screen": "not-granted"])
        precondition(privateValue(forwardPresenter, named: "flight") == nil,
                     "a partial grant must retire its active forward flight before restoring the card")
        precondition(forwardFlight.disposed && forwardFlight.disposeCount == 1,
                     "forward flight disposal must run once and tolerate synchronous completion")
        precondition(privateValue(forwardPresenter, named: "trackingTimer") == nil,
                     "partial grant restoration must stop Settings tracking")
        precondition(forwardPresenter.state == "pending",
                     "a synchronous stale forward completion cannot re-enter the helper state")

        let backPresenter = flightFixturePresenter(
            copy: copy,
            activePermission: "screen",
            needsInitialView: true,
        )
        backPresenter.startBackFlight()
        guard let backFlight = PermissionHostFlight.latest else {
            fatalError("Back transition must retain its active flight")
        }
        precondition(backFlight.started && backFlight.reverse && !backFlight.disposed,
                     "the fixture must exercise the reverse Back flight")
        precondition(privateValue(backPresenter, named: "backFlightTimer") is Timer,
                     "the reverse flight must own its fallback timer before the grant")
        backPresenter.updatePermissions(["accessibility": "not-granted", "screen": "granted"])
        precondition(privateValue(backPresenter, named: "flight") == nil,
                     "a partial grant during Back must retire its reverse flight")
        precondition(backFlight.disposed && backFlight.disposeCount == 1,
                     "reverse flight disposal must run once and tolerate synchronous completion")
        precondition(privateValue(backPresenter, named: "backFlightTimer") == nil,
                     "partial grant restoration must stop the Back timeout")
        precondition(!backPresenter.returning && backPresenter.state == "pending",
                     "a stale reverse completion cannot revive the helper after a grant")

        var dragEvents: [String] = []
        let dragPresenter = flightFixturePresenter(
            copy: copy,
            activePermission: "accessibility",
            needsInitialView: true,
            tracksDragSource: true,
            onEvent: { dragEvents.append($0) },
        )
        guard let dragSourcePanel = dragPresenter.helperPanel as? PermissionDragFixturePanel,
              let dragSourceView = dragPresenter.helperView else {
            fatalError("drag fixture must retain its source panel and view")
        }
        let dragView = PermissionHostDragView(frame: NSRect(x: 0, y: 0, width: 32, height: 32))
        dragSourceView.addSubview(dragView)
        dragPresenter.dragView = dragView
        dragPresenter.dragging = true
        dragPresenter.updatePermissions(["accessibility": "granted", "screen": "not-granted"])
        dragPresenter.handleAllow(permission: "screen")
        precondition(dragEvents.isEmpty && dragPresenter.state == "pending",
                     "a second permission cannot start before the active drag source ends")
        precondition(dragPresenter.terminalDragPanel === dragSourcePanel,
                     "partial grant must transfer an active drag source into terminal ownership")
        precondition(dragSourcePanel.closeCount == 0 && dragSourcePanel.alphaValue == 0,
                     "partial grant must hide, not close, the source panel before drag end")
        precondition(dragView.superview === dragSourceView && dragPresenter.dragging,
                     "partial grant must retain the drag view and session state until ended")
        dragPresenter.handleDragEnded()
        precondition(dragSourcePanel.closeCount == 1 && dragPresenter.terminalDragPanel == nil,
                     "the ended callback must close the retained source exactly once")
        precondition(!dragPresenter.dragging && dragPresenter.state == "pending",
                     "drag completion must leave the restored permission page pending")
        dragPresenter.handleAllow(permission: "screen")
        precondition(dragEvents == ["allow-screen"] && dragPresenter.state == "requesting",
                     "the next permission can start after the prior drag source ends")

        let visibleWindows = NSApplication.shared.windows.filter(\.isVisible)
        precondition(visibleWindows.isEmpty, "dual-row lifecycle smoke must keep all fixture windows unshown")
        forwardPresenter.close()
        backPresenter.close()
        dragPresenter.close()
        print("permission dual rows passed (independent cards, 98pt growth, snapshots, unshown flight fixtures)")
    }
}
