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
        print("permission SwiftUI host smoke passed")
    }
}
