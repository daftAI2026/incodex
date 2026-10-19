/**
 * [INPUT]: 依赖真实双行 Presenter、拖拽回调与合成 Settings 窗口定位器。
 * [OUTPUT]: 验证手动拖放收据、窗口坐标与 ManualSubmitted 静态回卡。
 * [POS]: 原生无窗隔离回归；不启动 ChatGPT、不访问 System Settings 或 TCC。
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

    init(
        view: NSView,
        frame: NSRect,
        radius: CGFloat,
        image: NSImage? = nil,
        captureImage: (() -> NSImage?)? = nil
    ) {
        self.view = view
        self.frame = frame
        self.radius = radius
        self.image = image
        self.captureImage = captureImage
    }
}

@MainActor
final class PermissionHostFlight {
    init(
        source: PermissionHostFlightEndpoint,
        target: @escaping () -> PermissionHostFlightEndpoint,
        reverse: Bool = false,
        isClosed: @escaping () -> Bool,
        onComplete: @escaping () -> Void,
        onError: @escaping (String) -> Void
    ) {}

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
    static var target: PermissionHostSettingsFrame?

    func locate() -> PermissionHostSettingsFrame? { Self.target }
    func prepareHandoff() {}
}

@MainActor
private final class ManualDropPanel: NSPanel {
    var closeCount = 0

    override func close() {
        closeCount += 1
        super.close()
    }
}

@MainActor
private final class ManualDropActionTarget: NSObject {
    @objc(allow:)
    func allow(_ sender: Any?) {}

    @objc(allowScreen:)
    func allowScreen(_ sender: Any?) {}

    @objc(resumeSettings:)
    func resumeSettings(_ sender: Any?) {}

    @objc(resumeScreenSettings:)
    func resumeScreenSettings(_ sender: Any?) {}
}

@MainActor
private struct ManualDropFixture {
    let presenter: PermissionHostPresenter
    let source: PermissionHostDragView
    let events: NSMutableArray
    let errors: NSMutableArray
    let panel: ManualDropPanel
    let initialView: IncodexPermissionInitialView
}

@MainActor
private func makeFixture(
    verification: String? = nil,
    activePermission: String = "screen",
    waitingForSettings: Bool = true,
    requested: Bool = true,
    closed: Bool = false,
    accessibilityStatus: String = "not-granted"
) -> ManualDropFixture {
    let copy: NSDictionary = [
        "title": "Update ChatGPT permissions",
        "body": "Check the official app permissions.",
        "permissionTitle": "Accessibility",
        "permissionDescription": "Read and control app interfaces.",
        "repair": "Allow",
        "later": "Back",
        "completeInSettings": "Complete in Settings",
        "screenTitle": "Screen & System Audio Recording",
        "screenDescription": "Allow ChatGPT to record screen content.",
        "screenAddedTitle": "Allow ChatGPT in System Settings",
        "screenAddedBody": "Select ChatGPT in Screen Settings.",
        "screenDragInstruction": "Drag ChatGPT into the list.",
        "screenDragInstructionRuns": "[]",
        "screenChecking": "Checking…",
        "screenUnknown": "Not verified",
        "done": "Done",
    ]
    let events = NSMutableArray()
    let errors = NSMutableArray()
    let presenter: PermissionHostPresenter
    if let verification {
        presenter = PermissionHostPresenter(
            copy: copy,
            layoutDirection: "leftToRight",
            screenVerification: verification,
            onEvent: { events.add($0) },
            onError: { errors.add($0) },
        )
    } else {
        presenter = PermissionHostPresenter(
            copy: copy,
            layoutDirection: "leftToRight",
            onEvent: { events.add($0) },
            onError: { errors.add($0) },
        )
    }
    presenter.configurePermissions([
        "accessibility": accessibilityStatus,
        "screen": "unknown",
    ])
    presenter.presented = true
    presenter.activePermission = activePermission
    presenter.state = waitingForSettings ? "awaiting-user" : "pending"
    presenter.closed = closed
    if requested { presenter.requestedPermissions.insert(activePermission) }

    let initialView = IncodexPermissionInitialView(frame: NSRect(x: 0, y: 0, width: 600, height: 400))
    initialView.configurePermissions(
        copy: copy,
        appIcon: NSImage(size: NSSize(width: 64, height: 64)),
        accessibilityIcon: NSImage(size: NSSize(width: 64, height: 64)),
        screenIcon: NSImage(size: NSSize(width: 64, height: 64)),
        actionTarget: ManualDropActionTarget(),
    )
    presenter.initialView = initialView
    presenter.cardView = initialView.permissionCardView(forPermission: activePermission)

    let panel = ManualDropPanel(
        contentRect: NSRect(x: -10_000, y: -10_000, width: 531, height: 110),
        styleMask: [.borderless],
        backing: .buffered,
        defer: false,
    )
    let helper = IncodexPermissionHelperView(frame: NSRect(x: 0, y: 0, width: 531, height: 110))
    panel.contentView = helper
    presenter.helperPanel = panel
    presenter.helperView = helper

    let source = PermissionHostDragView(frame: NSRect(x: 0, y: 0, width: 32, height: 32))
    source.owner = presenter
    presenter.dragView = source
    return ManualDropFixture(
        presenter: presenter,
        source: source,
        events: events,
        errors: errors,
        panel: panel,
        initialView: initialView,
    )
}

@MainActor
private func quartzPoint(for appKitPoint: NSPoint, mainScreen: NSRect) -> NSPoint {
    NSPoint(x: appKitPoint.x, y: mainScreen.maxY - appKitPoint.y)
}

@MainActor
private func locatorFrame(containing point: NSPoint, pid: pid_t = 41001, windowID: CGWindowID = 12) -> PermissionHostSettingsFrame {
    PermissionHostSettingsFrame(
        frame: NSRect(x: point.x - 3, y: point.y - 3, width: 6, height: 6),
        pid: pid,
        windowID: windowID,
    )
}

@MainActor
private func rows(in value: Any, depth: Int = 0) -> [PermissionRowState] {
    if let row = value as? PermissionRowState { return [row] }
    if let values = value as? [PermissionRowState] { return values }
    guard depth < 6 else { return [] }
    let mirror = Mirror(reflecting: value)
    guard String(describing: mirror.subjectType).contains("PermissionInitialState") else { return [] }
    return mirror.children.flatMap { rows(in: $0.value, depth: depth + 1) }
}

@MainActor
private func check(
    _ condition: @autoclosure () -> Bool,
    _ name: String,
    failures: inout [String]
) {
    if !condition() { failures.append(name) }
}

@MainActor
private func endedDrag(
    _ fixture: ManualDropFixture,
    point: NSPoint,
    operation: NSDragOperation = .copy,
    session: NSDraggingSession? = nil
) {
    let current = session ?? NSDraggingSession()
    fixture.presenter.dragging = true
    fixture.presenter.recordDragSession(current)
    fixture.source.draggingSession(current, endedAt: point, operation: operation)
}

@MainActor
@main
struct PermissionManualDropSmoke {
    static func main() {
        _ = NSApplication.shared
        var failures: [String] = []
        let mainScreen = NSScreen.screens.first?.frame
            ?? NSRect(x: 0, y: 0, width: 1440, height: 900)

        // ---- 合成显示器坐标矩阵；只读 locator fixture，不访问真实 Settings。 ----
        let points = [
            NSPoint(x: mainScreen.minX + 20, y: mainScreen.minY + 20),
            NSPoint(x: mainScreen.midX, y: mainScreen.midY),
            NSPoint(x: mainScreen.maxX - 20, y: mainScreen.maxY - 20),
            NSPoint(x: mainScreen.minX - 120, y: mainScreen.minY + 40),
            NSPoint(x: mainScreen.maxX + 90, y: mainScreen.maxY + 60),
        ]
        for (index, point) in points.enumerated() {
            let fixture = makeFixture(verification: "manual")
            SettingsLocator.target = locatorFrame(containing: quartzPoint(for: point, mainScreen: mainScreen))
            endedDrag(fixture, point: point)
            check(
                fixture.events.count == 1 && fixture.events.firstObject as? String == "settings-window-drop-received",
                "manual accepted drop emits one window receipt for coordinate case \(index)",
                failures: &failures,
            )
            endedDrag(fixture, point: point)
            check(fixture.events.count == 1, "one native drag session emits at most one receipt", failures: &failures)
            check(fixture.presenter.permissionStatuses["screen"] == "unknown",
                  "a drop receipt never grants Screen status", failures: &failures)
            fixture.presenter.close()
        }

        let center = NSPoint(x: mainScreen.midX, y: mainScreen.midY)
        let centerQuartz = quartzPoint(for: center, mainScreen: mainScreen)

        // ---- 普通 probe、取消、错误窗口和错误行必须拒绝收据。 ----
        let probe = makeFixture()
        SettingsLocator.target = locatorFrame(containing: centerQuartz)
        endedDrag(probe, point: center)
        check(probe.events.count == 0, "default probe mode rejects manual drop receipts", failures: &failures)
        probe.presenter.close()

        let cancelled = makeFixture(verification: "manual")
        SettingsLocator.target = locatorFrame(containing: centerQuartz)
        endedDrag(cancelled, point: center, operation: [])
        check(cancelled.events.count == 0, "cancelled drag never emits a receipt", failures: &failures)
        cancelled.presenter.close()

        let noSettingsWindow = makeFixture(verification: "manual")
        SettingsLocator.target = nil
        endedDrag(noSettingsWindow, point: center)
        check(noSettingsWindow.events.count == 0, "missing Settings window rejects the receipt", failures: &failures)
        noSettingsWindow.presenter.close()

        let wrongWindowFrame = makeFixture(verification: "manual")
        SettingsLocator.target = locatorFrame(containing: NSPoint(x: centerQuartz.x + 500, y: centerQuartz.y + 500))
        endedDrag(wrongWindowFrame, point: center)
        check(wrongWindowFrame.events.count == 0, "drop outside the current Settings window rejects the receipt", failures: &failures)
        wrongWindowFrame.presenter.close()

        let accessibilityRow = makeFixture(verification: "manual", activePermission: "accessibility")
        SettingsLocator.target = locatorFrame(containing: centerQuartz)
        endedDrag(accessibilityRow, point: center)
        check(accessibilityRow.events.count == 0, "Accessibility drag cannot emit a Screen receipt", failures: &failures)
        accessibilityRow.presenter.close()

        let notAwaiting = makeFixture(verification: "manual", waitingForSettings: false)
        SettingsLocator.target = locatorFrame(containing: centerQuartz)
        endedDrag(notAwaiting, point: center)
        check(notAwaiting.events.count == 0, "a pending card cannot emit a Settings receipt", failures: &failures)
        notAwaiting.presenter.close()

        let notRequested = makeFixture(verification: "manual", requested: false)
        SettingsLocator.target = locatorFrame(containing: centerQuartz)
        endedDrag(notRequested, point: center)
        check(notRequested.events.count == 0, "unrequested Screen settings cannot emit a receipt", failures: &failures)
        notRequested.presenter.close()

        let closed = makeFixture(verification: "manual", closed: true)
        SettingsLocator.target = locatorFrame(containing: centerQuartz)
        endedDrag(closed, point: center)
        check(closed.events.count == 0, "closed guide rejects a late drag callback", failures: &failures)
        closed.presenter.close()

        let stale = makeFixture(verification: "manual")
        let currentSession = NSDraggingSession()
        let oldSession = NSDraggingSession()
        stale.presenter.dragging = true
        stale.presenter.recordDragSession(currentSession)
        stale.source.draggingSession(oldSession, endedAt: center, operation: .copy)
        check(stale.events.count == 0, "a stale drag session cannot emit a receipt", failures: &failures)
        check(stale.presenter.dragging && stale.presenter.dragSession === currentSession,
              "a stale callback cannot retire the current drag session", failures: &failures)
        stale.presenter.close()

        // ---- 人工收据只恢复原双行；不设置 Screen 成功状态。 ----
        let submitted = makeFixture(verification: "manual", accessibilityStatus: "granted")
        submitted.presenter.setPermissionState("manual-submitted", permission: "screen", message: nil)
        let submittedRows = rows(in: submitted.initialView)
        let axRow = submittedRows.first(where: { $0.permissionTitle == "Accessibility" })
        let screenRow = submittedRows.first(where: { $0.permissionTitle == "Screen & System Audio Recording" })
        check(submitted.errors.count == 0, "manual-submitted is a supported non-error transition", failures: &failures)
        check(submitted.presenter.helperPanel == nil && submitted.presenter.state == "pending",
              "manual-submitted retires the Settings helper and returns to the card", failures: &failures)
        check(axRow?.permissionStatus == "granted", "AX evidence survives the Screen receipt", failures: &failures)
        check(screenRow?.permissionStatus == "unknown" && screenRow?.statusMessage == "Not verified",
              "the Screen row remains localized Unknown after the receipt", failures: &failures)
        check(screenRow?.settingsPlaceholder == false && screenRow?.allowEnabled == true,
              "the restored Screen row stays actionable without a success mark", failures: &failures)
        check(submitted.presenter.permissionStatuses["screen"] == "unknown",
              "Presenter aggregate status remains unknown", failures: &failures)
        submitted.presenter.close()
        SettingsLocator.target = nil

        guard failures.isEmpty else {
            failures.forEach { print("FAIL: \($0)") }
            fputs("\(failures.count) manual-drop contract failure(s)\n", stderr)
            exit(EXIT_FAILURE)
        }
        print("permission manual drop passed (manual receipt only; no TCC)")
    }
}
