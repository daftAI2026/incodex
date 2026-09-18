import AppKit
import CoreGraphics

/// A window returned by the System Settings locator.
///
/// `frame` uses Quartz global screen coordinates, matching the dictionaries
/// returned by `CGWindowListCopyWindowInfo`.  The presenter converts this
/// value to AppKit coordinates only when it positions its helper panel.
@MainActor
public struct PermissionHostSettingsFrame {
    public let frame: NSRect
    public let pid: pid_t
    public let windowID: CGWindowID

    public init(frame: NSRect, pid: pid_t, windowID: CGWindowID) {
        self.frame = frame
        self.pid = pid
        self.windowID = windowID
    }
}

/// Read-only System Settings window discovery used by the short-lived host.
///
/// This intentionally follows the existing dock-menu locator: it asks
/// LaunchServices for System Settings PIDs, then chooses the largest visible
/// layer-zero window owned by one of those PIDs.  It does not inspect titles,
/// accessibility elements, or user data, and it never launches or activates
/// System Settings.
@MainActor
public final class SettingsLocator {
    public static let bundleIdentifier = "com.apple.systempreferences"

    public init() {}

    public func locate() -> PermissionHostSettingsFrame? {
        precondition(Thread.isMainThread, "Settings location must run on the main thread")

        let applications = NSRunningApplication.runningApplications(
            withBundleIdentifier: Self.bundleIdentifier,
        )
        let pids = Set(
            applications
                .map(\.processIdentifier)
                .filter { $0 > 0 },
        )
        guard !pids.isEmpty else { return nil }

        let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
        guard let rawWindows = CGWindowListCopyWindowInfo(options, kCGNullWindowID)
            as? [[String: Any]] else {
            return nil
        }

        var best: PermissionHostSettingsFrame?
        var bestArea: CGFloat = 0
        for window in rawWindows {
            guard let ownerPID = number(window[kCGWindowOwnerPID as String]),
                  pids.contains(pid_t(ownerPID)),
                  number(window[kCGWindowLayer as String]) == 0,
                  let bounds = windowBounds(window[kCGWindowBounds as String]),
                  bounds.width > 0,
                  bounds.height > 0 else {
                continue
            }

            let area = bounds.width * bounds.height
            guard area.isFinite, area > bestArea,
                  let rawWindowID = number(window[kCGWindowNumber as String]) else {
                continue
            }
            bestArea = area
            best = PermissionHostSettingsFrame(
                frame: bounds,
                pid: pid_t(ownerPID),
                windowID: CGWindowID(rawWindowID),
            )
        }
        return best
    }

    private func number(_ value: Any?) -> Int? {
        if let value = value as? NSNumber { return value.intValue }
        if let value = value as? Int { return value }
        if let value = value as? Int32 { return Int(value) }
        if let value = value as? UInt32 { return Int(value) }
        return nil
    }

    private func windowBounds(_ value: Any?) -> NSRect? {
        guard let dictionary = value as? NSDictionary,
              let x = double(dictionary["X"]),
              let y = double(dictionary["Y"]),
              let width = double(dictionary["Width"]),
              let height = double(dictionary["Height"]),
              x.isFinite,
              y.isFinite,
              width.isFinite,
              height.isFinite else {
            return nil
        }
        return NSRect(x: x, y: y, width: width, height: height)
    }

    private func double(_ value: Any?) -> CGFloat? {
        if let value = value as? NSNumber { return CGFloat(value.doubleValue) }
        if let value = value as? Double { return CGFloat(value) }
        if let value = value as? CGFloat { return value }
        return nil
    }
}
