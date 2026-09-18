import AppKit
import CoreGraphics
import Foundation

private let permissionHostMaxLineBytes = 64 * 1024
#if INCODEX_PERMISSION_HOST_TESTING
private let permissionHostPresentationTimeout: TimeInterval = 0.15
#else
private let permissionHostPresentationTimeout: TimeInterval = 5 * 60
#endif
private let officialCodexBundleIdentifier = "com.openai.codex"
private let officialCodexExecutable = "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT"

/// The entry point deliberately owns protocol/process lifetime only. The
/// SwiftUI/AppKit surface is the single PermissionHostPresenter in the
/// adjacent presenter source, so the host cannot silently grow a second UI.
@MainActor
private enum PermissionHostPresentationGate {
    static func canPresentOfficialTarget() -> Bool {
#if INCODEX_PERMISSION_HOST_STUB
        // Only the protocol fixture compiles this entry with a no-window
        // presenter. This branch is absent from distributed executables.
        return true
#else
#if INCODEX_PERMISSION_HOST_TESTING
        if ProcessInfo.processInfo.environment["INCODEX_PERMISSION_HOST_DISABLE_PRESENTATION"] == "1" {
            return false
        }
#endif
        guard let front = NSWorkspace.shared.frontmostApplication,
              front.bundleIdentifier == officialCodexBundleIdentifier,
              front.executableURL?.path == officialCodexExecutable,
              front.isActive else { return false }

        let matches = NSWorkspace.shared.runningApplications.filter {
            $0.bundleIdentifier == officialCodexBundleIdentifier &&
                $0.executableURL?.path == officialCodexExecutable
        }
        guard matches.count == 1,
              matches[0].processIdentifier == front.processIdentifier else { return false }

        let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
        guard let windows = CGWindowListCopyWindowInfo(options, kCGNullWindowID)
            as? [[String: Any]] else { return false }
        return windows.contains { window in
            guard let owner = window[kCGWindowOwnerPID as String] as? NSNumber,
                  owner.intValue == front.processIdentifier,
                  let layer = window[kCGWindowLayer as String] as? NSNumber,
                  layer.intValue == 0,
                  let bounds = window[kCGWindowBounds as String] as? NSDictionary,
                  let width = bounds["Width"] as? NSNumber,
                  let height = bounds["Height"] as? NSNumber else { return false }
            return width.doubleValue.isFinite && height.doubleValue.isFinite &&
                width.doubleValue > 0 && height.doubleValue > 0
        }
#endif
    }
}

@MainActor
private final class PermissionHostProcess {
    private let nonce: String
    private var input = Data()
    private var configured = false
    private var ready = false
    private var finished = false
    private var presenter: PermissionHostPresenter?
    private var readSource: DispatchSourceRead?
    private var signalSources: [DispatchSourceSignal] = []
    private var presentationRetry: DispatchSourceTimer?
    private var presentationDeadline: DispatchWorkItem?
    private var presentationExpiresAt = 0.0

    init(nonce: String) { self.nonce = nonce }

    func start() {
        presentationExpiresAt = ProcessInfo.processInfo.systemUptime + permissionHostPresentationTimeout
        let deadline = DispatchWorkItem { [weak self] in
            guard let self, !self.finished else { return }
            self.protocolError("permission host presentation timed out")
        }
        presentationDeadline = deadline
        DispatchQueue.main.asyncAfter(deadline: .now() + permissionHostPresentationTimeout, execute: deadline)
        let source = DispatchSource.makeReadSource(
            fileDescriptor: FileHandle.standardInput.fileDescriptor,
            queue: .main,
        )
        source.setEventHandler { [weak self] in self?.readInput() }
        readSource = source
        source.resume()
        installSignal(SIGINT)
        installSignal(SIGTERM)
        installSignal(SIGHUP)
    }

    private func readInput() {
        guard !finished else { return }
        let data = FileHandle.standardInput.availableData
        guard !data.isEmpty else {
            finish(sendError: input.isEmpty ? nil : "permission host input ended mid-line")
            return
        }
        input.append(data)
        if input.count > permissionHostMaxLineBytes && !input.contains(10) {
            protocolError("permission host input line is too large")
            return
        }
        while let newline = input.firstIndex(of: 10) {
            let line = Data(input[..<newline])
            input.removeSubrange(...newline)
            if line.count > permissionHostMaxLineBytes {
                protocolError("permission host input line is too large")
                return
            }
            handle(line: line)
            if finished { return }
        }
        // A valid newline may be followed by an unterminated oversized line;
        // do not wait for another read before rejecting that bounded input.
        if input.count > permissionHostMaxLineBytes {
            protocolError("permission host input line is too large")
        }
    }

    private func handle(line: Data) {
        guard !line.isEmpty else {
            protocolError("permission host input is empty")
            return
        }
        guard let object = try? JSONSerialization.jsonObject(with: line),
              let message = object as? [String: Any] else {
            protocolError("permission host input is invalid")
            return
        }
        guard message["nonce"] as? String == nonce,
              let type = message["type"] as? String else {
            protocolError("permission host nonce or type is invalid")
            return
        }

        switch type {
        case "configure": handleConfigure(message)
        case "state": handleState(message)
        case "close":
            guard message.keys.allSatisfy({ $0 == "nonce" || $0 == "type" }) else {
                protocolError("permission host close message has extra fields")
                return
            }
            finish(sendError: nil)
        default:
            protocolError("permission host message type is unsupported")
        }
    }

    private func handleConfigure(_ message: [String: Any]) {
        guard !configured else {
            protocolError("permission host received duplicate configure")
            return
        }
        guard message.keys.allSatisfy({ $0 == "nonce" || $0 == "type" || $0 == "copy" || $0 == "layoutDirection" }),
              let rawCopy = message["copy"] as? [String: Any],
              let direction = message["layoutDirection"] as? String,
              direction == "leftToRight" || direction == "rightToLeft" else {
            protocolError("permission host configure message is invalid")
            return
        }

        let copy = NSMutableDictionary()
        for (key, value) in rawCopy {
            guard key.count <= 128, let string = value as? String, string.count <= 4096 else {
                protocolError("permission host copy is invalid")
                return
            }
            copy[key] = string
        }
        configured = true
        presenter = PermissionHostPresenter(
            copy: copy,
            layoutDirection: direction,
            onEvent: { [weak self] event in self?.send(type: event) },
            onError: { [weak self] message in self?.protocolError(message) },
        )
        attemptPresentation()
    }

    private func attemptPresentation() {
        guard configured, !finished, let presenter else { return }
        guard ProcessInfo.processInfo.systemUptime < presentationExpiresAt else {
            protocolError("permission host presentation timed out")
            return
        }
        guard PermissionHostPresentationGate.canPresentOfficialTarget() else {
            schedulePresentationRetry()
            return
        }
        let presented = presenter.present()
        guard !finished else { return }
        // A main-thread presentation can outlive the deadline while delaying
        // its queued timer. Never acknowledge a late native window as ready.
        guard ProcessInfo.processInfo.systemUptime < presentationExpiresAt else {
            protocolError("permission host presentation timed out")
            return
        }
        if presented {
            ready = true
            presentationRetry?.cancel()
            presentationRetry = nil
            presentationDeadline?.cancel()
            presentationDeadline = nil
            send(type: "ready")
            return
        }
        schedulePresentationRetry()
    }

    private func schedulePresentationRetry() {
        guard presentationRetry == nil, !finished else { return }
        let retry = DispatchSource.makeTimerSource(queue: .main)
        retry.schedule(deadline: .now() + .milliseconds(100), repeating: .milliseconds(100))
        retry.setEventHandler { [weak self] in self?.attemptPresentation() }
        presentationRetry = retry
        retry.resume()
    }

    private func handleState(_ message: [String: Any]) {
        guard configured, ready, let presenter,
              message.keys.allSatisfy({ ["nonce", "type", "state", "message"].contains($0) }),
              message["message"] == nil || message["message"] is String,
              let state = message["state"] as? String,
              ["repairing", "awaiting-user", "granted", "error"].contains(state) else {
            protocolError("permission host state message is invalid")
            return
        }
        if let message = message["message"] as? String, message.count > 512 {
            protocolError("permission host state message is too large")
            return
        }
        presenter.setState(state, message: message["message"] as? String)
    }

    private func installSignal(_ signal: Int32) {
        Darwin.signal(signal, SIG_IGN)
        let source = DispatchSource.makeSignalSource(signal: signal, queue: .main)
        source.setEventHandler { [weak self] in self?.finish(sendError: nil) }
        source.resume()
        signalSources.append(source)
    }

    private func send(type: String, message: String? = nil) {
        guard !finished else { return }
        var object: [String: Any] = ["nonce": nonce, "type": type]
        if let message { object["message"] = String(message.prefix(512)) }
        guard let data = try? JSONSerialization.data(withJSONObject: object),
              data.count + 1 <= permissionHostMaxLineBytes else {
            protocolError("permission host output is too large")
            return
        }
        var line = data
        line.append(10)
        FileHandle.standardOutput.write(line)
    }

    private func protocolError(_ message: String) {
        guard !finished else { return }
        send(type: "error", message: message)
        finish(sendError: nil)
    }

    private func finish(sendError: String?) {
        guard !finished else { return }
        if let sendError { send(type: "error", message: sendError) }
        finished = true
        presentationRetry?.cancel()
        presentationRetry = nil
        presentationDeadline?.cancel()
        presentationDeadline = nil
        readSource?.cancel()
        readSource = nil
        signalSources.forEach { $0.cancel() }
        signalSources.removeAll()
        presenter?.close()
        presenter = nil
        NSApplication.shared.terminate(nil)
    }
}

@main
@MainActor
private struct PermissionHostMain {
    static func main() {
        let arguments = Array(CommandLine.arguments.dropFirst())
        guard arguments.count == 2, arguments[0] == "--nonce",
              arguments[1].count == 32,
              arguments[1].allSatisfy({ $0.isHexDigit }) else {
            FileHandle.standardError.write(Data("permission host requires --nonce HEX32\n".utf8))
            Darwin.exit(2)
        }
        let application = NSApplication.shared
        application.setActivationPolicy(.accessory)
        application.finishLaunching()
        let host = PermissionHostProcess(nonce: arguments[1].lowercased())
        host.start()
        withExtendedLifetime(host) { application.run() }
    }
}
