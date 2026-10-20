/**
 * [INPUT]: 依赖权限行组件与 AppKit/SwiftUI 的无窗布局。
 * [OUTPUT]: 验证两实例的状态、动作与既有单行尺寸。
 * [POS]: 组件复用测试；不增加生产权限项或改变授权流程。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import AppKit
import SwiftUI

@MainActor
private final class LegacyActionTarget: NSObject {
    var allows = 0
    var resumes = 0
    @objc func allow(_ sender: Any?) { precondition(sender == nil); allows += 1 }
    @objc func resumeSettings(_ sender: Any?) { precondition(sender == nil); resumes += 1 }
}

@main
struct PermissionRowComponentSmoke {
    @MainActor
    static func main() {
        _ = NSApplication.shared
        var axAllows = 0
        var screenAllows = 0
        var axResumes = 0
        var screenResumes = 0
        let ax = PermissionRowState(
            title: "Accessibility", description: "Read app interfaces", buttonTitle: "Allow",
            onAllow: { axAllows += 1 }, onResumeSettings: { axResumes += 1 }
        )
        let screen = PermissionRowState(
            title: "Screenshots", description: "Read screen content", buttonTitle: "Allow",
            onAllow: { screenAllows += 1 }, onResumeSettings: { screenResumes += 1 }
        )
        let axHost = NSHostingView(rootView: PermissionCardRoot(state: ax))
        let screenHost = NSHostingView(rootView: PermissionCardRoot(state: screen))
        precondition(axHost.fittingSize == NSSize(width: 518, height: 80))
        precondition(screenHost.fittingSize == axHost.fittingSize)
        screen.setContent(allowEnabled: false, settingsPlaceholder: true)
        screen.placeholderHovered = true
        precondition(ax.allowEnabled && !ax.settingsPlaceholder && !ax.placeholderHovered)
        precondition(!screen.allowEnabled && screen.settingsPlaceholder && screen.placeholderHovered)
        precondition(ax.permissionTitle == "Accessibility" && screen.permissionTitle == "Screenshots")
        ax.performAllow()
        screen.setContent(allowEnabled: true, settingsPlaceholder: false)
        screen.performAllow()
        ax.resumeSettings()
        screen.resumeSettings()
        precondition(axAllows == 1 && screenAllows == 1 && axResumes == 1 && screenResumes == 1)
        precondition(!screen.placeholderHovered, "state transitions must retire placeholder hover")
        // 既有单卡仍用原 selector，行组件不持有页面或动作目标。
        let page = IncodexPermissionInitialView(frame: .zero)
        var target: LegacyActionTarget? = LegacyActionTarget()
        weak var weakTarget = target
        page.configure(copy: [
            "title": "Enable scripting", "body": "Allow access",
            "permissionTitle": "Accessibility", "permissionDescription": "Read interfaces",
            "repair": "Allow", "later": "Skip", "completeInSettings": "Complete in Settings",
            "layoutDirection": "rightToLeft",
        ], appIcon: nil, permissionIcon: nil, actionTarget: target)
        guard let pageState = Mirror(reflecting: page).children.first(where: { $0.label == "state" })?.value,
              let row = Mirror(reflecting: pageState).children.first(where: { $0.label == "row" })?.value as? PermissionRowState else {
            preconditionFailure("legacy single-card adapter must own one independent row")
        }
        precondition(row.permissionTitle == "Accessibility" && row.allow == "Allow")
        precondition(row.layoutDirection == .rightToLeft)
        row.performAllow()
        row.resumeSettings()
        precondition(target?.allows == 1 && target?.resumes == 1)
        row.placeholderHovered = true
        page.setContent(title: "Pending", body: "Finish in Settings", allowEnabled: false, settingsPlaceholder: true)
        precondition(!row.allowEnabled && row.settingsPlaceholder && !row.placeholderHovered)
        precondition(page.permissionCardView.fittingSize == axHost.fittingSize)
        target = nil
        precondition(weakTarget == nil, "legacy action target must remain weak")
        row.performAllow()
        row.resumeSettings()
        precondition(NSApplication.shared.windows.isEmpty, "component tests must not create windows")
        print("permission row component passed (independent state/actions; no windows)")
    }
}
