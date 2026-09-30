import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

@main
@MainActor
struct PermissionActivationPolicyAXSmoke {
    private static let backLabel = "Back"
    private static let pageTitle = "Enable ChatGPT scripting"

    static func main() {
        exit(run())
    }

    private static func run() -> Int32 {
        let application = NSApplication.shared
        guard application.setActivationPolicy(.accessory) else {
            return fail("could not set initial accessory policy")
        }
        application.finishLaunching()

        guard SettingsLocator().locate() != nil else {
            return fail("open System Settings to a visible window, then rerun the opt-in smoke")
        }

        var errors: [String] = []
        var events: [String] = []
        let presenter = PermissionHostPresenter(
            copy: copy,
            layoutDirection: "leftToRight",
            onEvent: { events.append($0) },
            onError: { errors.append($0) },
        )
        defer { presenter.close() }

        guard presenter.present() else {
            return fail("permission presenter did not create its initial page: \(errors.joined(separator: "; "))")
        }
        presenter.setState("awaiting-user")

        let helperAppeared = waitUntil(timeout: 20) {
            application.activationPolicy() == .prohibited && matchingBackButtons().count == 1
        }
        guard helperAppeared else {
            return fail("helper Back AXButton did not appear while policy was prohibited; policy=\(policyName(application.activationPolicy())) errors=\(errors)")
        }
        guard errors.isEmpty else {
            return fail("presenter reported an error before Back: \(errors.joined(separator: "; "))")
        }

        guard let back = matchingBackButtons().first else {
            return fail("helper Back AXButton disappeared before it could be inspected")
        }
        guard buttonTitles(back).contains(backLabel) else {
            return fail("helper Back AXButton did not expose its Back label")
        }
        guard copyAttribute(back, kAXEnabledAttribute as CFString) as? Bool == true else {
            return fail("helper Back AXButton was not enabled")
        }
        guard let backFrame = axFrame(back), let window = owningWindow(back),
              let windowFrame = axFrame(window), isVisible(back, frame: backFrame, windowFrame: windowFrame) else {
            return fail("helper Back AXButton did not have a visible frame in its owning window")
        }

        let relative = CGRect(
            x: backFrame.minX - windowFrame.minX,
            y: backFrame.minY - windowFrame.minY,
            width: backFrame.width,
            height: backFrame.height,
        )
        guard approximately(relative.minX, 18), approximately(relative.minY, 55),
              approximately(relative.width, 28), approximately(relative.height, 28) else {
            return fail("helper Back AXButton frame changed: \(relative)")
        }

        let press = AXUIElementPerformAction(back, kAXPressAction as CFString)
        guard press.rawValue == 0 else {
            return fail("AXPress on helper Back failed with AXError \(press.rawValue)")
        }

        let returned = waitUntil(timeout: 8) {
            application.activationPolicy() == .accessory &&
                !presenter.canStartDrag &&
                matchingBackButtons().isEmpty
        }
        guard returned else {
            return fail("Back did not restore accessory policy and dispose its helper; policy=\(policyName(application.activationPolicy())) canStartDrag=\(presenter.canStartDrag) backButtons=\(matchingBackButtons().count)")
        }
        guard errors.isEmpty else {
            return fail("presenter reported an error while returning from Back: \(errors.joined(separator: "; "))")
        }
        guard !events.contains("later"), containsAXElement(role: kAXStaticTextRole as String, label: pageTitle) else {
            return fail("Back did not leave the initial permission page available")
        }

        print("ACTIVATION_POLICY_AX prohibited=confirmed backLabel=Back backFrame=\(relative) AXPress=success restored=accessory helperDisposed=yes initialPage=yes")
        return 0
    }

    private static var copy: NSDictionary {
        [
            "title": pageTitle,
            "body": "Allow ChatGPT to access Accessibility.",
            "permissionTitle": "Accessibility",
            "permissionDescription": "Read and control app interfaces",
            "repair": "Allow",
            "later": "Skip",
            "back": backLabel,
            "addedTitle": "ChatGPT added",
            "addedBody": "ChatGPT is in the list.",
            "dragInstruction": "Drag ChatGPT into the app list above.",
            "completeInSettings": "Complete in Settings",
            "checking": "Checking",
            "repairing": "Preparing System Settings",
            "openSettings": "Open Settings",
            "errorTitle": "Permission needs attention",
            "errorBody": "The permission guide could not continue.",
        ] as NSDictionary
    }

    private static func waitUntil(timeout: TimeInterval, condition: () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        }
        return condition()
    }

    private static func matchingBackButtons() -> [AXUIElement] {
        let application = AXUIElementCreateApplication(getpid())
        var matches: [AXUIElement] = []
        var visited = 0
        collect(application, role: kAXButtonRole as String, label: backLabel, depth: 0, visited: &visited, into: &matches)
        return matches
    }

    private static func containsAXElement(role: String, label: String) -> Bool {
        let application = AXUIElementCreateApplication(getpid())
        var matches: [AXUIElement] = []
        var visited = 0
        collect(application, role: role, label: label, depth: 0, visited: &visited, into: &matches)
        return !matches.isEmpty
    }

    private static func collect(
        _ element: AXUIElement,
        role expectedRole: String,
        label expectedLabel: String,
        depth: Int,
        visited: inout Int,
        into matches: inout [AXUIElement],
    ) {
        guard depth <= 32, visited < 20_000 else { return }
        visited += 1
        let role = copyAttribute(element, kAXRoleAttribute as CFString) as? String
        if role == expectedRole && buttonTitles(element).contains(expectedLabel) {
            matches.append(element)
        }
        guard let children = copyAttribute(element, kAXChildrenAttribute as CFString) as? [AXUIElement] else { return }
        for child in children {
            collect(child, role: expectedRole, label: expectedLabel, depth: depth + 1, visited: &visited, into: &matches)
        }
    }

    private static func buttonTitles(_ element: AXUIElement) -> [String] {
        [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute].compactMap {
            copyAttribute(element, $0 as CFString) as? String
        }
    }

    private static func copyAttribute(_ element: AXUIElement, _ name: CFString) -> CFTypeRef? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, name, &value).rawValue == 0 else { return nil }
        return value
    }

    private static func owningWindow(_ element: AXUIElement) -> AXUIElement? {
        guard let rawWindow = copyAttribute(element, kAXWindowAttribute as CFString),
              CFGetTypeID(rawWindow) == AXUIElementGetTypeID() else { return nil }
        return unsafeBitCast(rawWindow, to: AXUIElement.self)
    }

    private static func axFrame(_ element: AXUIElement) -> CGRect? {
        guard let rawPosition = copyAttribute(element, kAXPositionAttribute as CFString),
              let rawSize = copyAttribute(element, kAXSizeAttribute as CFString),
              CFGetTypeID(rawPosition) == AXValueGetTypeID(),
              CFGetTypeID(rawSize) == AXValueGetTypeID() else { return nil }
        let position = unsafeBitCast(rawPosition, to: AXValue.self)
        let size = unsafeBitCast(rawSize, to: AXValue.self)
        var origin = CGPoint.zero
        var extent = CGSize.zero
        guard AXValueGetValue(position, .cgPoint, &origin), AXValueGetValue(size, .cgSize, &extent),
              origin.x.isFinite, origin.y.isFinite, extent.width.isFinite, extent.height.isFinite,
              extent.width > 0, extent.height > 0 else { return nil }
        return CGRect(origin: origin, size: extent)
    }

    private static func isVisible(_ button: AXUIElement, frame buttonFrame: CGRect, windowFrame: CGRect) -> Bool {
        guard copyAttribute(button, kAXHiddenAttribute as CFString) as? Bool != true,
              windowFrame.insetBy(dx: -2, dy: -2).contains(buttonFrame) else { return false }
        guard let rows = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
            as? [[String: Any]] else { return false }
        return rows.contains { row in
            guard (row[kCGWindowOwnerPID as String] as? NSNumber)?.intValue == Int(getpid()),
                  (row[kCGWindowAlpha as String] as? NSNumber)?.doubleValue ?? 0 > 0,
                  let bounds = row[kCGWindowBounds as String] as? NSDictionary,
                  let x = (bounds["X"] as? NSNumber)?.doubleValue,
                  let y = (bounds["Y"] as? NSNumber)?.doubleValue,
                  let width = (bounds["Width"] as? NSNumber)?.doubleValue,
                  let height = (bounds["Height"] as? NSNumber)?.doubleValue else { return false }
            let visibleFrame = CGRect(x: x, y: y, width: width, height: height)
            return visibleFrame.insetBy(dx: -2, dy: -2).contains(buttonFrame)
        }
    }

    private static func approximately(_ lhs: CGFloat, _ rhs: CGFloat) -> Bool {
        abs(lhs - rhs) <= 1
    }

    private static func policyName(_ policy: NSApplication.ActivationPolicy) -> String {
        switch policy {
        case .regular: return "regular"
        case .accessory: return "accessory"
        case .prohibited: return "prohibited"
        @unknown default: return "unknown"
        }
    }

    private static func fail(_ message: String) -> Int32 {
        fputs("ACTIVATION_POLICY_AX_FAIL \(message)\n", stderr)
        return 1
    }
}
