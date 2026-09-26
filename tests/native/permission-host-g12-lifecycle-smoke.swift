import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

@MainActor
private final class EventSink {
    var events: [String] = []
    var errors: [String] = []
}

@main
@MainActor
struct PermissionHostG12LifecycleSmoke {
    private static let copy: NSDictionary = [
        "title": "Enable ChatGPT scripting",
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
    ]

    static func main() {
        do { try run() }
        catch {
            FileHandle.standardError.write(Data("G12 lifecycle smoke failed: \(error)\n".utf8))
            exit(1)
        }
    }

    private static func run() throws {
        let application = NSApplication.shared
        guard application.setActivationPolicy(.accessory) else {
            throw SmokeFailure.message("could not set accessory activation policy")
        }
        application.finishLaunching()
        guard SettingsLocator().locate() != nil else {
            throw SmokeFailure.message("open the real System Settings main window before the opt-in test")
        }
        guard visibleWindowCount() == 0 else {
            throw SmokeFailure.message("the test process unexpectedly started with a visible window")
        }
        try verifyNormalCloseAndRepeatedClose()
        try verifyErrorCleanup()
        try verifyGrantedCleanup()
        guard visibleWindowCount() == 0, application.activationPolicy() == .accessory else {
            throw SmokeFailure.message("visible windows remained or activation policy was not restored")
        }
        print("G12_LIFECYCLE close=zero-windows-repeat-safe arrowTimer=nil lateWindow=none error=one-guide-window flightDisposed=yes helperArrow=zero granted=zero-windows flightDisposed=yes timers=nil")
    }

    private static func verifyNormalCloseAndRepeatedClose() throws {
        let flow = try startFlow()
        try waitUntil("visible helper/arrow and scheduled arrow timer", timeout: 10) {
            let helper = stored("helperPanel", in: flow.presenter) as? NSPanel
            let arrow = stored("arrowPanel", in: flow.presenter) as? NSPanel
            return helper?.isVisible == true && arrow?.isVisible == true &&
                stored("flight", in: flow.presenter) == nil &&
                stored("trackingTimer", in: flow.presenter) != nil &&
                stored("arrowTimer", in: flow.presenter) != nil
        }
        guard visibleWindowCount() >= 3 else {
            throw SmokeFailure.message("awaiting-user did not expose initial, helper, and arrow windows")
        }
        flow.presenter.close()
        try assertPresenterTimersCleared(flow.presenter, label: "ordinary close")
        guard visibleWindowCount() == 0 else {
            throw SmokeFailure.message("ordinary close left \(visibleWindowCount()) visible windows")
        }
        flow.presenter.close()
        try assertPresenterTimersCleared(flow.presenter, label: "repeated close")
        guard visibleWindowCount() == 0, flow.sink.events == ["allow"] else {
            throw SmokeFailure.message("repeated close changed windows or emitted an extra event")
        }
        RunLoop.current.run(until: Date().addingTimeInterval(0.7))
        guard visibleWindowCount() == 0 else {
            throw SmokeFailure.message("a delayed arrow callback reopened a window after close")
        }
    }

    private static func verifyErrorCleanup() throws {
        let flow = try startFlow()
        try waitUntil("visible helper and pending arrow timer before error", timeout: 10) {
            let helper = stored("helperPanel", in: flow.presenter) as? NSPanel
            let arrow = stored("arrowPanel", in: flow.presenter) as? NSPanel
            return helper?.isVisible == true && arrow?.isVisible == true &&
                stored("flight", in: flow.presenter) == nil &&
                stored("arrowTimer", in: flow.presenter) != nil
        }
        flow.presenter.setState("error")
        try waitUntil("one visible error guide", timeout: 2) { visibleWindowCount() == 1 }
        guard visibleWindowCount() == 1,
              (stored("initialPanel", in: flow.presenter) as? NSWindow)?.isVisible == true,
              stored("helperPanel", in: flow.presenter) == nil,
              stored("arrowPanel", in: flow.presenter) == nil,
              stored("flight", in: flow.presenter) == nil else {
            throw SmokeFailure.message("error did not keep only the guide while clearing helper/arrow/flight")
        }
        try assertPresenterTimersCleared(flow.presenter, label: "error transition")
        guard flow.sink.errors.isEmpty, flow.sink.events == ["allow"] else {
            throw SmokeFailure.message("error transition emitted an unexpected protocol event")
        }
        flow.presenter.close()
        flow.presenter.close()
        guard visibleWindowCount() == 0 else {
            throw SmokeFailure.message("closing the error page left a visible window")
        }
    }

    private static func verifyGrantedCleanup() throws {
        let flow = try startFlow()
        let flight = try waitForActiveFlight(flow.presenter)
        flow.presenter.setState("granted")
        try waitUntil("granted closes all presenter windows", timeout: 2) { visibleWindowCount() == 0 }
        guard visibleWindowCount() == 0,
              stored("initialPanel", in: flow.presenter) == nil,
              stored("helperPanel", in: flow.presenter) == nil,
              stored("arrowPanel", in: flow.presenter) == nil,
              stored("flight", in: flow.presenter) == nil else {
            throw SmokeFailure.message("granted did not release all presenter windows and flight reference")
        }
        try assertPresenterTimersCleared(flow.presenter, label: "granted transition")
        try assertFlightDisposed(flight, label: "granted transition")
        guard flow.sink.errors.isEmpty, flow.sink.events == ["allow"] else {
            throw SmokeFailure.message("granted transition emitted an unexpected protocol event")
        }
    }

    private static func startFlow() throws -> (presenter: PermissionHostPresenter, sink: EventSink) {
        let sink = EventSink()
        let presenter = PermissionHostPresenter(
            copy: copy,
            layoutDirection: "leftToRight",
            onEvent: { sink.events.append($0) },
            onError: { sink.errors.append($0) },
        )
        guard presenter.present(), visibleWindowCount() == 1 else {
            throw SmokeFailure.message("normal initial page did not create exactly one visible window: \(sink.errors)")
        }
        guard let initialView = stored("initialView", in: presenter),
              let viewState = stored("state", in: initialView),
              let actionTarget = stored("actionTarget", in: viewState) as? NSObject else {
            throw SmokeFailure.message("could not resolve the initial SwiftUI state and its native action target")
        }
        let allowSelector = Selector("allow:")
        guard actionTarget.responds(to: allowSelector) else {
            throw SmokeFailure.message("resolved native action target does not implement allow:")
        }
        guard NSApplication.shared.sendAction(allowSelector, to: actionTarget, from: nil) else {
            throw SmokeFailure.message("NSApplication did not dispatch the native allow: action")
        }
        guard sink.events == ["allow"] else {
            throw SmokeFailure.message("native Allow action dispatched but did not settle exactly one event: \(sink.events)")
        }
        presenter.setState("repairing")
        presenter.setState("awaiting-user")
        return (presenter, sink)
    }

    private static func waitForActiveFlight(_ presenter: PermissionHostPresenter) throws -> PermissionHostFlight {
        var active: PermissionHostFlight?
        try waitUntil("live forward screen flight", timeout: 5) {
            guard let candidate = stored("flight", in: presenter) as? PermissionHostFlight,
                  let entries = stored("entries", in: candidate) as? [Any],
                  !entries.isEmpty else { return false }
            let clock = stored("clock", in: candidate) as? IncodexPermissionDisplayLink
            guard stored("timer", in: candidate) is Timer || clock?.displayLinked == true else { return false }
            active = candidate
            return true
        }
        guard let active else {
            throw SmokeFailure.message("no active flight/timer; Reduce Motion=\(NSWorkspace.shared.accessibilityDisplayShouldReduceMotion)")
        }
        try assertFlightActive(active)
        return active
    }

    private static func assertFlightActive(_ flight: PermissionHostFlight) throws {
        let entries = stored("entries", in: flight) as? [Any] ?? []
        let clock = stored("clock", in: flight) as? IncodexPermissionDisplayLink
        guard !entries.isEmpty, stored("disposed", in: flight) as? Bool == false,
              stored("timer", in: flight) is Timer || clock?.displayLinked == true else {
            throw SmokeFailure.message("forward flight was not active before granted")
        }
    }

    private static func assertFlightDisposed(_ flight: PermissionHostFlight, label: String) throws {
        let entries = stored("entries", in: flight) as? [Any] ?? []
        let clock = stored("clock", in: flight) as? IncodexPermissionDisplayLink
        guard stored("disposed", in: flight) as? Bool == true, entries.isEmpty,
              stored("timer", in: flight) == nil, clock?.displayLinked == false,
              stored("images", in: flight) == nil else {
            throw SmokeFailure.message("\(label) left a live flight timer, entry, image, or display clock")
        }
    }

    private static func assertPresenterTimersCleared(_ presenter: PermissionHostPresenter, label: String) throws {
        for name in ["trackingTimer", "arrowTimer", "arrowReturnTimer", "backFlightTimer"] {
            guard stored(name, in: presenter) == nil else {
                throw SmokeFailure.message("\(label) left presenter timer \(name) active")
            }
        }
        guard stored("flight", in: presenter) == nil else {
            throw SmokeFailure.message("\(label) retained its active flight")
        }
    }

    private static func visibleWindowCount() -> Int {
        NSApplication.shared.windows.filter(\.isVisible).count
    }

    private static func waitUntil(_ label: String, timeout: TimeInterval, condition: () -> Bool) throws {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() { return }
            RunLoop.current.run(until: Date().addingTimeInterval(0.02))
        }
        if condition() { return }
        throw SmokeFailure.message("timed out waiting for \(label); visible windows=\(visibleWindowCount())")
    }

    private static func stored(_ name: String, in object: Any) -> Any? {
        var mirror: Mirror? = Mirror(reflecting: object)
        while let current = mirror {
            if let value = current.children.first(where: { $0.label == name })?.value {
                return unwrapped(value)
            }
            mirror = current.superclassMirror
        }
        return nil
    }

    private static func unwrapped(_ value: Any) -> Any? {
        var value = value
        while Mirror(reflecting: value).displayStyle == .optional {
            guard let child = Mirror(reflecting: value).children.first else { return nil }
            value = child.value
        }
        return value
    }
}

private enum SmokeFailure: Error, CustomStringConvertible {
    case message(String)
    var description: String {
        if case let .message(message) = self { return message }
        return "unknown failure"
    }
}
