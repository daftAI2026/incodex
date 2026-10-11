/**
 * [INPUT]: 依赖权限行组件与 AppKit/SwiftUI 的无窗布局。
 * [OUTPUT]: 验证两实例的状态、动作与既有单行尺寸。
 * [POS]: 组件复用测试；不增加生产权限项或改变授权流程。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import AppKit
import SwiftUI

@main
struct PermissionRowComponentSmoke {
    @MainActor
    static func main() {
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
        precondition(NSApplication.shared.windows.isEmpty, "component tests must not create windows")
        print("permission row component passed (independent state/actions; no windows)")
    }
}
