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
    @objc func allow(_ sender: Any?) {}
    @objc func allowScreen(_ sender: Any?) {}
    @objc func resumeSettings(_ sender: Any?) {}
    @objc func resumeScreenSettings(_ sender: Any?) {}
}

@MainActor
@main
struct PermissionDualRowsSmoke {
    static func main() {
        _ = NSApplication.shared
        let copy: NSDictionary = [
            "title": "Enable private access",
            "body": "Grant both permissions before opening a private window.",
            "permissionTitle": "Accessibility",
            "permissionDescription": "Read and control app interfaces",
            "repair": "Allow",
            "later": "Back",
            "completeInSettings": "Complete in Settings",
            "screenTitle": "Screen Recording",
            "screenDescription": "Capture the current display for this window.",
            "screenAddedTitle": "Add Incodex",
            "screenAddedBody": "Select Incodex in Screen Recording settings.",
            "screenDragInstruction": "Drag Incodex into the app list above.",
            "screenDragInstructionRuns": "Drag Incodex into the app list above.",
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
            title: "Enable private access",
            body: "Grant both permissions before opening a private window.",
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
        presenter.configurePermissions(["accessibility": "granted", "screen": "not-granted"])
        presenter.updatePermissions(["accessibility": "unknown", "screen": "granted"])
        presenter.setPermissionState("repairing", permission: "screen", message: nil)
        precondition(events.isEmpty, "status reconciliation must not emit Allow or retry actions")
        precondition(NSApplication.shared.windows.isEmpty, "dual-row smoke must not create native windows")
        print("permission dual rows passed (independent cards, 98pt growth, snapshots, windowless presenter)")
    }
}
