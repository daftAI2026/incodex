/**
 * [INPUT]: 依赖 permission-views、permission-initial 与 presenter 的双权限接口。
 * [OUTPUT]: 验证双行尺寸、独立快照和权限状态入口，不创建窗口。
 * [POS]: 双权限卡无窗契约；不访问宿主应用、系统设置或 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import AppKit
import Foundation
import SwiftUI

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
    init(source: PermissionHostFlightEndpoint, target: @escaping () -> PermissionHostFlightEndpoint, reverse: Bool = false, isClosed: @escaping () -> Bool, onComplete: @escaping () -> Void, onError: @escaping (String) -> Void) {}
    func start() {}
    func dispose() {}
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
@main
struct PermissionDualRowsSmoke {
    static func main() {
        _ = NSApplication.shared
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
        precondition(accessibilityRow.permissionState == "unknown" && screenRow.permissionState == "unknown",
                     "unverified TCC state must stay unknown")
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
        accessibilityRow.permissionState = "granted"
        screenRow.permissionState = "not-granted"
        precondition(!accessibilityRow.allowEnabled && accessibilityRow.settingsPlaceholder)
        precondition(screenRow.allowEnabled && !screenRow.settingsPlaceholder)
        precondition(accessibilityRow.permissionState == "granted" && screenRow.permissionState == "not-granted",
                     "one permission update must not overwrite the other row")

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
        presenter.updatePermissions(["accessibility": "unknown", "screen": "granted"])
        presenter.updatePermissions(["accessibility": "not-granted", "screen": "not-granted"])
        presenter.setPermissionState("granted", permission: "accessibility", message: nil)
        precondition(!privateBool(presenter, named: "closed"), "one granted row must not close the whole card")
        presenter.setPermissionState("granted", permission: "screen", message: nil)
        precondition(privateBool(presenter, named: "closed"), "the card closes after both rows are granted")
        precondition(events.isEmpty, "status reconciliation must not emit Allow or retry actions")
        precondition(NSApplication.shared.windows.isEmpty, "dual-row smoke must not create native windows")
        print("permission dual rows passed (independent cards, 98pt growth, snapshots, windowless presenter)")
    }
}
