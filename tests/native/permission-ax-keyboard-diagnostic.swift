import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

/// Opt-in evidence collector for the current native permission guide.
///
/// This is diagnostic output, not a parity assertion. It does not change TCC,
/// touch an installed bundle, or toggle VoiceOver / keyboard accessibility.
@main
@MainActor
struct PermissionAXKeyboardDiagnostic {
    private static let pageTitle = "Enable ChatGPT scripting"
    private static var dispatchedKeyboardEvents = 0
    private static let outputHandle: FileHandle? = {
        guard let argument = CommandLine.arguments.first(where: { $0.hasPrefix("--diagnostic-output=") }) else {
            return nil
        }
        let path = String(argument.dropFirst("--diagnostic-output=".count))
        return FileHandle(forWritingAtPath: path)
    }()

    @MainActor
    private final class Recorder {
        var errors: [String] = []
        var events: [String] = []
    }

    private enum ProbeKey: String, CaseIterable {
        case tab = "Tab"
        case reverseTab = "Shift+Tab"
        case `return` = "Return"
        case space = "Space"

        var keyCode: CGKeyCode {
            switch self {
            case .tab, .reverseTab: 48
            case .return: 36
            case .space: 49
            }
        }

        var flags: CGEventFlags {
            self == .reverseTab ? .maskShift : []
        }
    }

    static func main() {
        exit(run())
    }

    private static func run() -> Int32 {
        let application = NSApplication.shared
        guard application.setActivationPolicy(.accessory) else {
            return fail("could not set initial accessory policy")
        }
        application.finishLaunching()

        // Only read an already-visible Settings window. This fixture never
        // opens Settings or changes any Settings control.
        guard SettingsLocator().locate() != nil else {
            return fail("open an existing System Settings window, then rerun the opt-in diagnostic")
        }

        let recorder = Recorder()
        let presenter = PermissionHostPresenter(
            copy: copy,
            layoutDirection: "leftToRight",
            onEvent: { recorder.events.append($0) },
            onError: { recorder.errors.append($0) },
        )
        defer { presenter.close() }

        guard presenter.present() else {
            return fail("permission presenter did not create its initial page: \(recorder.errors.joined(separator: "; "))")
        }

        guard waitUntil(timeout: 5, condition: {
            matchingButtons(label: "Allow").count == 1 && matchingButtons(label: "Skip").count == 1
        }) else {
            return fail("initial AX controls were not available: \(recorder.errors.joined(separator: "; "))")
        }
        dumpTree(state: "initial", callbacks: recorder.events)
        probeKeyboard(state: "initial", keys: ProbeKey.allCases, recorder: recorder)

        presenter.setState("awaiting-user")
        let helperAppeared = waitUntil(timeout: 20, condition: {
            application.activationPolicy() == .prohibited &&
                matchingButtons(label: "Back").count == 1 &&
                matchingButtons(label: "Complete in Settings").count == 1
        })
        guard helperAppeared else {
            return fail("helper and placeholder AX controls did not appear: policy=\(policyName(application.activationPolicy())) errors=\(recorder.errors.joined(separator: "; "))")
        }
        dumpTree(state: "helper-prohibited-placeholder", callbacks: recorder.events)

        // The keyboard probe has an in-process key-window/frontmost guard. If
        // Settings owns the keyboard during .prohibited, it records a skip and
        // sends no key to Settings.
        probeKeyboard(
            state: "helper-prohibited-placeholder",
            keys: [.tab, .return, .space],
            recorder: recorder,
        )

        guard let back = matchingButtons(label: "Back").first else {
            return fail("Back button disappeared before state transition")
        }
        let beforeBack = recorder.events.count
        let backResult = AXUIElementPerformAction(back, kAXPressAction as CFString)
        emit([
            "kind": "STATE_TRANSITION",
            "input": "AXPress (setup only; not keyboard or VoiceOver evidence)",
            "target": "Back",
            "axError": backResult.rawValue,
            "callbacksBefore": beforeBack,
            "callbacksAfterImmediateDispatch": recorder.events.count,
        ])
        guard backResult.rawValue == AXError.success.rawValue else {
            return fail("AXPress setup transition on Back failed with AXError \(backResult.rawValue)")
        }

        let returned = waitUntil(timeout: 10, condition: {
            application.activationPolicy() == .accessory &&
                matchingButtons(label: "Back").isEmpty &&
                matchingButtons(label: "Allow").count == 1 &&
                matchingButtons(label: "Skip").count == 1
        })
        guard returned else {
            return fail("Back did not restore the initial page: policy=\(policyName(application.activationPolicy())) errors=\(recorder.errors.joined(separator: "; "))")
        }
        dumpTree(state: "after-back", callbacks: recorder.events)
        probeKeyboard(state: "after-back", keys: ProbeKey.allCases, recorder: recorder)

        guard recorder.errors.isEmpty else {
            return fail("presenter reported errors: \(recorder.errors.joined(separator: "; "))")
        }

        emit([
            "kind": "DIAGNOSTIC_LIMITS",
            "voiceOver": "not toggled or tested",
            "tcc": "not changed or reset",
            "systemSettingsControls": "not changed",
            "processAccessibilityTrusted": AXIsProcessTrusted(),
            "postEventAccessAvailable": CGPreflightPostEventAccess(),
            "keyboardEvents": "CGEvent only when frontmost, non-prohibited, key-window, and CGPreflightPostEventAccess is true; no permission prompt requested",
            "referenceParity": "not asserted",
            "callbacks": eventCounts(recorder.events),
        ])
        presenter.close()
        emit(["kind": "CLEANUP", "closed": true, "activationPolicy": policyName(application.activationPolicy())])
        emit(["kind": "DIAGNOSTIC_COMPLETED", "exitCode": 0])
        try? outputHandle?.synchronize()
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
            "back": "Back",
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

    private static func dumpTree(state: String, callbacks: [String]) {
        emit(["kind": "AX_TREE_BEGIN", "state": state, "activationPolicy": policyName(NSApplication.shared.activationPolicy())])
        let application = AXUIElementCreateApplication(getpid())
        var visited = 0
        collect(application, path: "app", depth: 0, state: state, visited: &visited)
        emit(["kind": "AX_TREE_END", "state": state, "nodes": visited])

        let appFocused = axElement(application, kAXFocusedUIElementAttribute as CFString)
        let systemFocused = axElement(AXUIElementCreateSystemWide(), kAXFocusedUIElementAttribute as CFString)
        emit([
            "kind": "FOCUS_SNAPSHOT",
            "state": state,
            "applicationActive": NSApplication.shared.isActive,
            "frontmostPID": NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1,
            "keyWindow": NSApplication.shared.keyWindow?.title ?? NSNull(),
            "applicationFocusedElement": summary(appFocused),
            "systemFocusedElement": summary(systemFocused),
            "callbacks": eventCounts(callbacks),
        ])
    }

    private static func collect(
        _ element: AXUIElement,
        path: String,
        depth: Int,
        state: String,
        visited: inout Int,
    ) {
        guard depth <= 40, visited < 20_000 else { return }
        visited += 1

        var actions: CFArray?
        let actionResult = AXUIElementCopyActionNames(element, &actions)
        let node: [String: Any] = [
            "kind": "AX_NODE",
            "state": state,
            "path": path,
            "role": stringAttribute(element, kAXRoleAttribute as CFString) ?? NSNull(),
            "subrole": stringAttribute(element, kAXSubroleAttribute as CFString) ?? NSNull(),
            "title": stringAttribute(element, kAXTitleAttribute as CFString) ?? NSNull(),
            "description": stringAttribute(element, kAXDescriptionAttribute as CFString) ?? NSNull(),
            "help": stringAttribute(element, kAXHelpAttribute as CFString) ?? NSNull(),
            "identifier": stringAttribute(element, kAXIdentifierAttribute as CFString) ?? NSNull(),
            "value": scalarValue(attribute(element, kAXValueAttribute as CFString)) ?? NSNull(),
            "enabled": boolAttribute(element, kAXEnabledAttribute as CFString) as Any? ?? NSNull(),
            "focused": boolAttribute(element, kAXFocusedAttribute as CFString) as Any? ?? NSNull(),
            "selected": boolAttribute(element, kAXSelectedAttribute as CFString) as Any? ?? NSNull(),
            "hidden": boolAttribute(element, kAXHiddenAttribute as CFString) as Any? ?? NSNull(),
            "frame": frame(element) ?? NSNull(),
            "actions": [String](actions as? [String] ?? []),
            "actionsResult": actionResult.rawValue,
        ]
        emit(node)

        guard let children = attribute(element, kAXChildrenAttribute as CFString) as? [AXUIElement] else { return }
        for (index, child) in children.enumerated() {
            collect(child, path: "\(path).\(index)", depth: depth + 1, state: state, visited: &visited)
        }
    }

    private static func probeKeyboard(state: String, keys: [ProbeKey], recorder: Recorder) {
        for key in keys {
            let before = recorder.events.count
            let dispatchedBefore = dispatchedKeyboardEvents
            let focusBefore = summary(axElement(AXUIElementCreateSystemWide(), kAXFocusedUIElementAttribute as CFString))
            let frontmostPID = NSWorkspace.shared.frontmostApplication?.processIdentifier ?? -1
            let activationPolicy = NSApplication.shared.activationPolicy()
            guard frontmostPID == getpid(),
                  NSApplication.shared.isActive,
                  NSApplication.shared.keyWindow != nil,
                  activationPolicy != .prohibited else {
                emit([
                    "kind": "KEY_PROBE",
                    "state": state,
                    "key": key.rawValue,
                    "delivery": "not-sent; diagnostic process was not frontmost in a non-prohibited state with a key window",
                    "frontmostPID": frontmostPID,
                    "applicationActive": NSApplication.shared.isActive,
                    "activationPolicy": policyName(activationPolicy),
                    "hasKeyWindow": NSApplication.shared.keyWindow != nil,
                    "focusBefore": focusBefore,
                    "focusAfter": focusBefore,
                    "callbacksBefore": before,
                    "callbacksAfter": recorder.events.count,
                ])
                continue
            }
            guard CGPreflightPostEventAccess() else {
                emit([
                    "kind": "KEY_PROBE",
                    "state": state,
                    "key": key.rawValue,
                    "delivery": "not-sent; CGPreflightPostEventAccess=false (no permission prompt requested)",
                    "frontmostPID": frontmostPID,
                    "applicationActive": NSApplication.shared.isActive,
                    "hasKeyWindow": NSApplication.shared.keyWindow != nil,
                    "focusBefore": focusBefore,
                    "focusAfter": focusBefore,
                    "callbacksBefore": before,
                    "callbacksAfter": recorder.events.count,
                    "keyboardEventsDispatched": 0,
                ])
                continue
            }

            guard let down = CGEvent(keyboardEventSource: nil, virtualKey: key.keyCode, keyDown: true),
                  let up = CGEvent(keyboardEventSource: nil, virtualKey: key.keyCode, keyDown: false) else {
                emit([
                    "kind": "KEY_PROBE",
                    "state": state,
                    "key": key.rawValue,
                    "delivery": "event-construction-failed",
                    "focusBefore": focusBefore,
                    "callbacksBefore": before,
                    "callbacksAfter": recorder.events.count,
                ])
                continue
            }
            down.flags = key.flags
            up.flags = key.flags
            down.post(tap: .cghidEventTap)
            pumpApplicationEvents(for: 0.12)
            up.post(tap: .cghidEventTap)
            pumpApplicationEvents(for: 0.12)

            let focusAfter = summary(axElement(AXUIElementCreateSystemWide(), kAXFocusedUIElementAttribute as CFString))
            emit([
                "kind": "KEY_PROBE",
                "state": state,
                "key": key.rawValue,
                "delivery": dispatchedKeyboardEvents > dispatchedBefore
                    ? "CGEvent posted and keyboard event observed by NSApplication"
                    : "CGEvent posted but no keyboard event observed by NSApplication",
                "focusBefore": focusBefore,
                "focusAfter": focusAfter,
                "callbacksBefore": before,
                "callbacksAfter": recorder.events.count,
                "callbackDelta": recorder.events.count - before,
                "keyboardEventsPosted": 2,
                "keyboardEventsDispatched": dispatchedKeyboardEvents - dispatchedBefore,
                "callbackCounts": eventCounts(recorder.events),
            ])
        }
    }

    private static func pumpApplicationEvents(for duration: TimeInterval) {
        let application = NSApplication.shared
        let deadline = Date().addingTimeInterval(duration)
        while Date() < deadline {
            guard let event = application.nextEvent(
                matching: .any,
                until: deadline,
                inMode: .default,
                dequeue: true,
            ) else { break }
            if event.type == .keyDown || event.type == .keyUp { dispatchedKeyboardEvents += 1 }
            application.sendEvent(event)
        }
    }

    private static func matchingButtons(label: String) -> [AXUIElement] {
        let application = AXUIElementCreateApplication(getpid())
        var matches: [AXUIElement] = []
        var visited = 0
        collectMatches(application, label: label, depth: 0, visited: &visited, into: &matches)
        return matches
    }

    private static func collectMatches(
        _ element: AXUIElement,
        label: String,
        depth: Int,
        visited: inout Int,
        into matches: inout [AXUIElement],
    ) {
        guard depth <= 40, visited < 20_000 else { return }
        visited += 1
        let role = stringAttribute(element, kAXRoleAttribute as CFString)
        let labels = [
            stringAttribute(element, kAXTitleAttribute as CFString),
            stringAttribute(element, kAXDescriptionAttribute as CFString),
            stringAttribute(element, kAXValueAttribute as CFString),
        ].compactMap { $0 }
        if role == kAXButtonRole as String && labels.contains(label) { matches.append(element) }
        for child in (attribute(element, kAXChildrenAttribute as CFString) as? [AXUIElement] ?? []) {
            collectMatches(child, label: label, depth: depth + 1, visited: &visited, into: &matches)
        }
    }

    private static func attribute(_ element: AXUIElement, _ name: CFString) -> CFTypeRef? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, name, &value).rawValue == AXError.success.rawValue else { return nil }
        return value
    }

    private static func axElement(_ element: AXUIElement, _ name: CFString) -> AXUIElement? {
        guard let value = attribute(element, name), CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
        return unsafeBitCast(value, to: AXUIElement.self)
    }

    private static func stringAttribute(_ element: AXUIElement, _ name: CFString) -> String? {
        attribute(element, name) as? String
    }

    private static func boolAttribute(_ element: AXUIElement, _ name: CFString) -> Bool? {
        attribute(element, name) as? Bool
    }

    private static func scalarValue(_ value: CFTypeRef?) -> Any? {
        guard let value else { return nil }
        if let string = value as? String { return string }
        if let number = value as? NSNumber { return number }
        return nil
    }

    private static func frame(_ element: AXUIElement) -> [Double]? {
        guard let rawPosition = attribute(element, kAXPositionAttribute as CFString),
              let rawSize = attribute(element, kAXSizeAttribute as CFString),
              CFGetTypeID(rawPosition) == AXValueGetTypeID(),
              CFGetTypeID(rawSize) == AXValueGetTypeID() else { return nil }
        let position = unsafeBitCast(rawPosition, to: AXValue.self)
        let size = unsafeBitCast(rawSize, to: AXValue.self)
        var origin = CGPoint.zero
        var extent = CGSize.zero
        guard AXValueGetValue(position, .cgPoint, &origin), AXValueGetValue(size, .cgSize, &extent) else { return nil }
        return [origin.x, origin.y, extent.width, extent.height].map(Double.init)
    }

    private static func summary(_ element: AXUIElement?) -> [String: Any] {
        guard let element else { return ["available": false] }
        return [
            "available": true,
            "role": stringAttribute(element, kAXRoleAttribute as CFString) ?? NSNull(),
            "title": stringAttribute(element, kAXTitleAttribute as CFString) ?? NSNull(),
            "description": stringAttribute(element, kAXDescriptionAttribute as CFString) ?? NSNull(),
            "help": stringAttribute(element, kAXHelpAttribute as CFString) ?? NSNull(),
            "focused": boolAttribute(element, kAXFocusedAttribute as CFString) as Any? ?? NSNull(),
            "frame": frame(element) ?? NSNull(),
        ]
    }

    private static func eventCounts(_ events: [String]) -> [String: Int] {
        Dictionary(grouping: events, by: { $0 }).mapValues(\.count)
    }

    private static func emit(_ value: [String: Any]) {
        guard JSONSerialization.isValidJSONObject(value),
              let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]),
              let line = String(data: data, encoding: .utf8) else {
            print("DIAGNOSTIC_ENCODING_ERROR")
            return
        }
        print(line)
        if let outputHandle {
            try? outputHandle.seekToEnd()
            try? outputHandle.write(contentsOf: Data((line + "\n").utf8))
        }
    }

    private static func policyName(_ policy: NSApplication.ActivationPolicy) -> String {
        switch policy {
        case .regular: "regular"
        case .accessory: "accessory"
        case .prohibited: "prohibited"
        @unknown default: "unknown"
        }
    }

    private static func fail(_ message: String) -> Int32 {
        emit(["kind": "DIAGNOSTIC_SETUP_FAILED", "message": message])
        return 1
    }
}
