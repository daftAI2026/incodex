import AppKit
import SwiftUI

@main
enum PermissionViewsSmoke {
    @MainActor
    static func main() {
        _ = NSApplication.shared
        let view = IncodexPermissionFlightView(frame: NSRect(x: 0, y: 0, width: 518, height: 80))
        precondition(NSStringFromClass(type(of: view)) == "IncodexPermissionFlightView")
        precondition(view.subviews.count == 1)
        let host = view.subviews[0]
        precondition(String(describing: type(of: host)).contains("NSHostingView"))
        let source = NSImage(size: NSSize(width: 518, height: 80))
        let target = NSImage(size: NSSize(width: 531, height: 110))
        view.setSourceImage(source, targetImage: target)
        for (progress, radius) in [(0.0, 24.0), (0.5, 18.0), (1.0, 12.0)] {
            view.updateProgress(progress, cornerRadius: radius, reduceTransparency: false)
            view.setFrameSize(NSSize(width: 526, height: 100))
            view.layoutSubtreeIfNeeded()
            precondition(view.subviews[0] === host, "keep one SwiftUI host across display ticks")
            precondition(host.frame == view.bounds)
            precondition(view.flightProgress == progress)
            precondition(view.flightCornerRadius == radius)
            precondition(source.size == NSSize(width: 518, height: 80))
            precondition(target.size == NSSize(width: 531, height: 110))
        }
        view.updateProgress(-1, cornerRadius: -1, reduceTransparency: true)
        precondition(view.flightProgress == 0 && view.flightCornerRadius == 0)
        view.updateProgress(2, cornerRadius: 12, reduceTransparency: false)
        precondition(view.flightProgress == 1)
        view.updateProgress(.nan, cornerRadius: .infinity, reduceTransparency: false)
        precondition(view.flightProgress == 1 && view.flightCornerRadius == 12)
        precondition(view.hitTest(NSPoint(x: 50, y: 30)) == nil)
        view.setSourceImage(nil, targetImage: nil)
        let copy: NSDictionary = [
            "title": "Enable ChatGPT scripting", "body": "Allow Accessibility access.",
            "permissionTitle": "Accessibility", "permissionDescription": "Read and control app interfaces",
            "repair": "Allow", "later": "Skip", "completeInSettings": "Complete in Settings",
            "back": "Back", "dragInstruction": "Drag ChatGPT into the app list above.",
        ]
        let initial = IncodexPermissionInitialView(frame: NSRect(x: 0, y: 0, width: 600, height: 340))
        initial.configure(copy: copy, appIcon: source, permissionIcon: target, actionTarget: nil)
        let initialHost = initial.subviews[0]
        precondition(String(describing: type(of: initialHost)).contains("NSHostingView"))
        precondition(String(describing: type(of: initial.permissionCardView)).contains("NSHostingView"))
        precondition(initial.preferredContentSize.width == 600)
        precondition(initial.preferredContentSize.height >= 312)
        initial.setContent(title: "Error", body: "Retry after fixing permissions.", allowEnabled: false, settingsPlaceholder: true)
        initial.layoutSubtreeIfNeeded()
        precondition(initial.subviews[0] === initialHost)
        precondition(initial.permissionCardView.bounds.size == NSSize(width: 518, height: 80))
        let helper = IncodexPermissionHelperView(frame: NSRect(x: 0, y: 0, width: 531, height: 110))
        helper.configure(copy: copy, appIcon: source, actionTarget: nil)
        precondition(helper.preferredContentSize.width == 531)
        precondition(helper.preferredContentSize.height >= 110)
        precondition(helper.appRowFrame.size == NSSize(width: 459, height: 42))
        precondition(String(describing: type(of: helper.appRowView)).contains("NSHostingView"))
        precondition(String(describing: type(of: helper.subviews[0])).contains("NSHostingView"))
        print("permission SwiftUI host smoke passed")
    }
}
