import Foundation
import OSAKit

/// Host/protocol adapter only. The script embeds the existing presenter and
/// motion modules verbatim; this class must not own windows or animation.
@MainActor
final class PermissionHostPresenter {
    private var script: OSAScript?
    private var poller: DispatchSourceTimer?
    private var closed = false
    private var failed = false
    private let onEvent: (String) -> Void
    private let onError: (String) -> Void

    init(copy: NSDictionary, layoutDirection: String,
         onEvent: @escaping (String) -> Void, onError: @escaping (String) -> Void) {
        self.onEvent = onEvent
        self.onError = onError
        guard let language = OSALanguage(forName: "JavaScript") else {
            fail("System JavaScript language is unavailable")
            return
        }
        let instance = OSAScript(source: PermissionHostOSASource.runtime, language: language)
        script = instance
        var error: NSDictionary?
        _ = instance.executeAndReturnError(&error)
        if let error { fail(error.description); return }
        guard let data = try? JSONSerialization.data(withJSONObject: ["copy": copy, "layoutDirection": layoutDirection]),
              let json = String(data: data, encoding: .utf8) else {
            fail("Cannot encode permission host configuration")
            return
        }
        _ = invoke("configure", json)
        guard !failed else { return }
        let timer = DispatchSource.makeTimerSource(queue: .main)
        timer.schedule(deadline: .now(), repeating: .milliseconds(20))
        timer.setEventHandler { [weak self] in self?.drain() }
        poller = timer
        timer.resume()
    }

    func present() -> Bool {
        guard !closed, !failed else { return false }
        _ = invoke("present")
        // OSA executes Promise jobs when the previous JS invocation unwinds.
        // Recheck now, before the outer official-target gate loses focus.
        let ready = invoke("isready")?.booleanValue == true
        drain()
        return ready && !failed && !closed
    }

    func setState(_ state: String, message: String?) {
        guard !closed, !failed else { return }
        var object: [String: Any] = ["state": state]
        if let message { object["message"] = message }
        guard let data = try? JSONSerialization.data(withJSONObject: object),
              let json = String(data: data, encoding: .utf8) else {
            fail("Cannot encode permission host state")
            return
        }
        _ = invoke("setstate", json)
    }

    func close() {
        guard !closed else { return }
        closed = true
        poller?.cancel()
        poller = nil
        if let script {
            var error: NSDictionary?
            _ = script.executeHandler(withName: "close", arguments: [], error: &error)
            if let error, !failed { fail(error.description) }
        }
        script = nil
    }

    private func invoke(_ name: String, _ argument: String? = nil) -> NSAppleEventDescriptor? {
        guard let script, !failed else { return nil }
        var error: NSDictionary?
        let arguments = argument.map { [NSAppleEventDescriptor(string: $0)] } ?? []
        let result = script.executeHandler(withName: name, arguments: arguments, error: &error)
        if let error { fail(error.description); return nil }
        return result
    }

    private func drain() {
        guard !closed, !failed,
              let text = invoke("drainEvents")?.stringValue else { return }
        guard text.utf8.count <= 64 * 1024,
              let data = text.data(using: .utf8),
              let events = (try? JSONSerialization.jsonObject(with: data)) as? [[String: Any]],
              events.count <= 32 else {
            fail("Invalid permission host events")
            return
        }
        for event in events {
            guard !closed, !failed else { return }
            guard let type = event["type"] as? String else { fail("Invalid permission event"); return }
            if type == "error" { fail(event["message"] as? String ?? "Permission UI failed"); return }
            guard ["allow", "retry", "later", "close"].contains(type) else { fail("Unsupported permission event"); return }
            onEvent(type)
        }
    }

    private func fail(_ message: String) {
        guard !failed else { return }
        failed = true
        poller?.cancel()
        poller = nil
        onError(String(message.prefix(512)))
    }
}
