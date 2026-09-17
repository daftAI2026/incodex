import AppKit
import CoreGraphics
import Darwin
import SwiftUI

private let copy: NSDictionary = [
    "title": "Enable ChatGPT scripting",
    "body": "Allow Accessibility access.",
    "permissionTitle": "Accessibility",
    "permissionDescription": "Read and control app interfaces",
    "repair": "Allow",
    "later": "Skip",
    "completeInSettings": "Complete in Settings",
    "back": "Back",
    "dragInstruction": "Drag ChatGPT into the app list above.",
]

private func report(_ text: String) {
    print(text)
    fflush(stdout)
}

private func pump(_ seconds: TimeInterval) {
    let deadline = Date().addingTimeInterval(seconds)
    while deadline.timeIntervalSinceNow > 0 {
        if let event = NSApp.nextEvent(
            matching: .any,
            until: deadline,
            inMode: .default,
            dequeue: true,
        ) {
            NSApp.sendEvent(event)
        } else {
            RunLoop.current.run(mode: .default, before: deadline)
        }
    }
}

private func nestedValue(named label: String, in value: Any, depth: Int = 0) -> Any? {
    guard depth < 12 else { return nil }
    let mirror = Mirror(reflecting: value)
    for child in mirror.children {
        if child.label == label { return child.value }
        if let found = nestedValue(named: label, in: child.value, depth: depth + 1) {
            return found
        }
    }
    if let superclass = mirror.superclassMirror {
        for child in superclass.children {
            if child.label == label { return child.value }
            if let found = nestedValue(named: label, in: child.value, depth: depth + 1) {
                return found
            }
        }
    }
    return nil
}

private struct BoolLeaf {
    let path: String
    let value: Bool
}

private func boolLeavesInside(_ value: Any, path: String = "wrapper", depth: Int = 0) -> [BoolLeaf] {
    if let bool = value as? Bool { return [BoolLeaf(path: path, value: bool)] }
    guard depth < 12 else { return [] }
    var leaves: [BoolLeaf] = []
    for child in Mirror(reflecting: value).children {
        let label = child.label ?? "<unlabeled>"
        leaves.append(contentsOf: boolLeavesInside(child.value, path: "\(path).\(label)", depth: depth + 1))
    }
    return leaves
}

private func placeholderHovered(in view: IncodexPermissionInitialView) -> Bool? {
    guard let state = nestedValue(named: "state", in: view) else { return nil }
    guard let published = nestedValue(named: "_placeholderHovered", in: state)
        ?? nestedValue(named: "placeholderHovered", in: state) else { return nil }
    let currentValues = boolLeavesInside(published).filter { $0.path.hasSuffix(".currentValue") }
    guard currentValues.count == 1 else {
        report("HOVER_MIRROR invalidCurrentValueCount=\(currentValues.count)")
        return nil
    }
    report("HOVER_MIRROR currentValuePath=\(currentValues[0].path) value=\(currentValues[0].value)")
    return currentValues[0].value
}

private func sendMouseMoved(to point: NSPoint, window: NSWindow) {
    guard let primary = NSScreen.screens.first else { exit(2) }
    let screenPoint = window.convertPoint(toScreen: point)
    let quartzPoint = CGPoint(x: screenPoint.x, y: primary.frame.maxY - screenPoint.y)
    guard let event = CGEvent(
        mouseEventSource: nil, mouseType: .mouseMoved,
        mouseCursorPosition: quartzPoint, mouseButton: .left,
    ) else { exit(2) }
    event.post(tap: .cghidEventTap)
}

private func ownPanelIsOnScreen(_ panel: NSPanel) -> Bool {
    guard panel.isVisible, panel.windowNumber > 0 else {
        report("HOVER_WINDOW invalidVisible=\(panel.isVisible) windowNumber=\(panel.windowNumber)")
        return false
    }
    let numberKey = kCGWindowNumber as String
    let ownerPIDKey = kCGWindowOwnerPID as String
    guard let windows = CGWindowListCopyWindowInfo(
        [.optionOnScreenOnly, .excludeDesktopElements],
        kCGNullWindowID,
    ) as? [[String: Any]] else {
        report("HOVER_WINDOW onScreenInfoUnavailable")
        return false
    }
    for info in windows {
        guard let number = (info[numberKey] as? NSNumber)?.intValue,
              number == panel.windowNumber else { continue }
        let pid = (info[ownerPIDKey] as? NSNumber)?.intValue ?? -1
        report("HOVER_WINDOW number=\(number) ownerPID=\(pid) expectedPID=\(getpid())")
        return pid == getpid()
    }
    report("HOVER_WINDOW panelWindow=\(panel.windowNumber) absentFromOnScreenList")
    return false
}

@main
@MainActor
struct PermissionPlaceholderHoverSmoke {
    static func main() {
        let application = NSApplication.shared
        application.setActivationPolicy(.accessory)
        application.finishLaunching()
        NSApp.activate(ignoringOtherApps: true)

        let view = IncodexPermissionInitialView(frame: NSRect(x: 0, y: 0, width: 600, height: 340))
        view.configure(copy: copy, appIcon: nil, permissionIcon: nil, actionTarget: nil)
        view.setContent(
            title: "Awaiting Accessibility",
            body: "Waiting for permission.",
            allowEnabled: true,
            settingsPlaceholder: true,
        )
        let panel = NSPanel(
            contentRect: NSRect(x: 240, y: 520, width: 600, height: 340),
            styleMask: [.titled],
            backing: .buffered,
            defer: false,
        )
        panel.isReleasedWhenClosed = false
        panel.acceptsMouseMovedEvents = true
        panel.contentView = view
        panel.makeKeyAndOrderFront(nil)
        panel.orderFrontRegardless()
        panel.displayIfNeeded()
        pump(0.35)

        guard ownPanelIsOnScreen(panel) else {
            panel.orderOut(nil)
            panel.close()
            report("HOVER_PROBE_INVALID own panel was not on screen")
            exit(2)
        }

        let cardRect = view.permissionCardView.convert(view.permissionCardView.bounds, to: nil)
        let outside = NSPoint(x: cardRect.minX - 20, y: cardRect.minY - 20)
        let center = NSPoint(x: cardRect.midX, y: cardRect.midY)
        report("HOVER_TARGET cardInWindow=\(cardRect) center=\(center) outside=\(outside)")
        sendMouseMoved(to: outside, window: panel)
        pump(0.1)
        sendMouseMoved(to: center, window: panel)
        pump(0.2)
        let afterHover = placeholderHovered(in: view)
        guard afterHover == true else {
            panel.orderOut(nil)
            panel.close()
            report("HOVER_PROBE_INVALID real placeholder hover did not become true")
            exit(2)
        }

        view.setContent(
            title: "Returned",
            body: "Back returned to the permission page.",
            allowEnabled: true,
            settingsPlaceholder: false,
        )
        // Read synchronously, before any event-pump callback can hide the
        // placeholder or legitimately re-enter it.
        let afterHide = placeholderHovered(in: view)

        view.setContent(
            title: "Awaiting Again",
            body: "Waiting again.",
            allowEnabled: true,
            settingsPlaceholder: true,
        )
        sendMouseMoved(to: outside, window: panel)
        pump(0.1)
        sendMouseMoved(to: center, window: panel)
        pump(0.2)
        guard placeholderHovered(in: view) == true else {
            panel.orderOut(nil)
            panel.close()
            report("HOVER_PROBE_INVALID second real placeholder hover did not become true")
            exit(2)
        }
        view.configure(copy: copy, appIcon: nil, permissionIcon: nil, actionTarget: nil)
        // configure reuses the same state object and must synchronously clear
        // stale hover before any re-display can generate a new onHover(true).
        let afterConfigure = placeholderHovered(in: view)

        report("HOVER_RESET afterHover=\(String(describing: afterHover)) afterHide=\(String(describing: afterHide)) afterConfigure=\(String(describing: afterConfigure))")
        panel.orderOut(nil)
        panel.close()

        guard afterHide == false, afterConfigure == false else {
            report("HOVER_CONTRACT_FAIL setContent/configure did not synchronously clear hover")
            exit(1)
        }
        report("placeholder hover reset smoke passed")
    }
}
