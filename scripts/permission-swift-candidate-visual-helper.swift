import AppKit
import ApplicationServices
import CoreGraphics
import Foundation
import ScreenCaptureKit

private struct VisualToolError: Error, LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

@main
private struct PermissionSwiftCandidateVisualHelper {
    @MainActor
    static func main() async {
        do {
            try await run(Array(CommandLine.arguments.dropFirst()))
        } catch {
            writeJSON(["ok": false, "error": error.localizedDescription])
            Darwin.exit(2)
        }
    }

    @MainActor
    private static func run(_ arguments: [String]) async throws {
        guard let command = arguments.first else {
            throw VisualToolError(message: "expected preflight, app, windows, Settings window diagnostics, press, press-settings-placeholder, or snapshot")
        }
        switch command {
        case "preflight":
            let operatingSystem = ProcessInfo.processInfo.operatingSystemVersion
            let screenCapture = CGPreflightScreenCaptureAccess()
            let accessibility = AXIsProcessTrusted()
            writeJSON([
                "ok": true,
                "command": "preflight",
                "screenCaptureAlreadyGranted": screenCapture,
                "accessibilityAlreadyGranted": accessibility,
                "postEventAccessAlreadyGranted": CGPreflightPostEventAccess(),
                "osVersion": "\(operatingSystem.majorVersion).\(operatingSystem.minorVersion).\(operatingSystem.patchVersion)",
                "wallTime": ISO8601DateFormatter().string(from: Date()),
                "monotonicSeconds": ProcessInfo.processInfo.systemUptime,
            ])
        case "app":
            guard arguments.count == 3 else {
                throw VisualToolError(message: "app requires BUNDLE_ID EXECUTABLE_PATH")
            }
            let expectedBundle = arguments[1]
            let expectedExecutable = URL(fileURLWithPath: arguments[2]).standardizedFileURL.path
            let matches = NSWorkspace.shared.runningApplications.filter {
                $0.bundleIdentifier == expectedBundle &&
                    $0.executableURL?.standardizedFileURL.path == expectedExecutable
            }
            let windows = windowRecords().filter { record in
                guard let pid = record["ownerPID"] as? Int else { return false }
                return matches.contains { $0.processIdentifier == pid_t(pid) }
            }
            writeJSON([
                "ok": true,
                "command": "app",
                "bundleIdentifier": expectedBundle,
                "executablePath": expectedExecutable,
                "matchingPIDs": matches.map { Int($0.processIdentifier) },
                "activePIDs": matches.filter(\.isActive).map { Int($0.processIdentifier) },
                "frontmostPID": NSWorkspace.shared.frontmostApplication.map { Int($0.processIdentifier) } as Any? ?? NSNull(),
                "windows": windows,
                "wallTime": ISO8601DateFormatter().string(from: Date()),
                "monotonicSeconds": ProcessInfo.processInfo.systemUptime,
            ])
        case "windows":
            writeJSON([
                "ok": true,
                "command": "windows",
                "frontmostPID": NSWorkspace.shared.frontmostApplication.map { Int($0.processIdentifier) } as Any? ?? NSNull(),
                "windows": windowRecords(),
                "wallTime": ISO8601DateFormatter().string(from: Date()),
                "monotonicSeconds": ProcessInfo.processInfo.systemUptime,
            ])
        case "settings-window":
            try reportExactSettingsWindow(arguments)
        case "settings-move":
            try moveExactSettingsWindow(arguments)
        case "settings-restore-position":
            try restoreExactSettingsWindowPosition(arguments)
        case "settings-close":
            try closeExactSettingsWindow(arguments, duringFlightForHostPID: nil)
        case "settings-close-during-flight":
            guard arguments.count == 9, let hostPID = pid_t(arguments[8]) else {
                throw VisualToolError(message: "settings-close-during-flight requires PID EXECUTABLE WINDOW_ID X Y WIDTH HEIGHT FLIGHT_HOST_PID")
            }
            try closeExactSettingsWindow(arguments, duringFlightForHostPID: hostPID)
        case "press":
            try pressButton(arguments, settingsPlaceholderDiagnostic: false)
        case "press-settings-placeholder":
            try pressButton(arguments, settingsPlaceholderDiagnostic: true)
        case "keyboard-allow":
            try activateInitialAllowByKeyboard(arguments)
        case "snapshot":
            guard arguments.count == 2 else {
                throw VisualToolError(message: "snapshot requires OUTPUT_PNG_PATH")
            }
            let result = try await captureSnapshot(URL(fileURLWithPath: arguments[1]).standardizedFileURL)
            writeJSON(result)
        case "series":
            guard arguments.count == 5,
                  let frameCount = Int(arguments[3]), (1...60).contains(frameCount),
                  let intervalMilliseconds = Int(arguments[4]), (60...2_000).contains(intervalMilliseconds) else {
                throw VisualToolError(message: "series requires OUTPUT_DIRECTORY SAFE_LABEL FRAME_COUNT INTERVAL_MS (1..60 frames, 60..2000ms)")
            }
            let outputDirectory = URL(fileURLWithPath: arguments[1], isDirectory: true).standardizedFileURL
            let label = arguments[2]
                guard !label.isEmpty, label.unicodeScalars.allSatisfy({ $0.isASCII && (CharacterSet.alphanumerics.contains($0) || $0 == "-" || $0 == "_") }) else {
                    throw VisualToolError(message: "series label may contain only ASCII letters, digits, hyphen, and underscore")
                }
            var previousStart: Double?
            var previousFinish: Double?
            for index in 0..<frameCount {
                let suffix = String(format: "%03d", index)
                let path = outputDirectory.appendingPathComponent("\(label)-\(suffix).png")
                let result = try await captureSnapshot(path)
                var record = result
                record["seriesIndex"] = index
                record["seriesLabel"] = label
                if let currentStart = result["monotonicStartedSeconds"] as? Double {
                    if let previousStart {
                        record["intervalFromPreviousCaptureStartSeconds"] = currentStart - previousStart
                    }
                    if let previousFinish {
                        record["gapAfterPreviousCaptureFinishSeconds"] = currentStart - previousFinish
                    }
                    previousStart = currentStart
                }
                previousFinish = result["monotonicFinishedSeconds"] as? Double
                writeJSON(record)
                if index + 1 < frameCount {
                    try await Task.sleep(nanoseconds: UInt64(intervalMilliseconds) * 1_000_000)
                }
            }
        default:
            throw VisualToolError(message: "unsupported helper command: \(command)")
        }
    }

    @MainActor
    private static func pressButton(_ arguments: [String], settingsPlaceholderDiagnostic: Bool) throws {
        guard arguments.count == 3, let pid = pid_t(arguments[1]) else {
            let command = settingsPlaceholderDiagnostic ? "press-settings-placeholder" : "press"
            throw VisualToolError(message: "\(command) requires PID EXACT_AX_BUTTON_LABEL")
        }
        guard AXIsProcessTrusted() else {
            throw VisualToolError(message: "Accessibility control is not already authorized; no prompt was requested")
        }
        let title = arguments[2]
        let appElement = AXUIElementCreateApplication(pid)
        let windows = copyAttribute(appElement, kAXWindowsAttribute as CFString) as? [AXUIElement] ?? []
        var buttons: [AXUIElement] = []
        var visitCount = 0
        for window in windows {
            collectButtons(window, expectedTitle: title, depth: 0, visitCount: &visitCount, into: &buttons)
        }
        guard buttons.count == 1, let button = buttons.first else {
            throw VisualToolError(message: "expected exactly one AXButton labeled \(title.debugDescription) in PID \(pid); found \(buttons.count)")
        }
        var actionNames: CFArray?
        let actionNamesError = AXUIElementCopyActionNames(button, &actionNames)
        let actionNameList = [String](actionNames as? [String] ?? [])
        let buttonRole = copyAttribute(button, kAXRoleAttribute as CFString) as? String
        let buttonTitle = copyAttribute(button, kAXTitleAttribute as CFString) as? String
        let buttonDescription = copyAttribute(button, kAXDescriptionAttribute as CFString) as? String
        let buttonValue = copyAttribute(button, kAXValueAttribute as CFString)
        if settingsPlaceholderDiagnostic {
            guard buttonRole == (kAXButtonRole as String) else {
                throw VisualToolError(message: "Settings placeholder target is not an AXButton")
            }
            guard actionNameList.contains(kAXPressAction as String) else {
                throw VisualToolError(message: "Settings placeholder AXButton does not expose AXPress")
            }
        }
        guard buttonRole == (kAXButtonRole as String) else {
            throw VisualToolError(message: "AXPress target is not an AXButton")
        }
        guard actionNameList.contains(kAXPressAction as String) else {
            throw VisualToolError(message: "AXButton does not expose AXPress")
        }
        guard copyAttribute(button, kAXEnabledAttribute as CFString) as? Bool == true else {
            throw VisualToolError(message: "unique AXButton \(title.debugDescription) in PID \(pid) is not enabled")
        }
        // AppKit may omit AXHidden for a visible control. An explicit true is
        // disqualifying; the owning-window geometry and z-order checks below
        // establish visibility when the attribute is unsupported.
        if copyAttribute(button, kAXHiddenAttribute as CFString) as? Bool == true {
            throw VisualToolError(message: "unique AXButton \(title.debugDescription) in PID \(pid) is hidden")
        }
        guard let rawAXWindow = copyAttribute(button, kAXWindowAttribute as CFString),
              CFGetTypeID(rawAXWindow) == AXUIElementGetTypeID() else {
            throw VisualToolError(message: "unique AXButton \(title.debugDescription) has no valid owning AXWindow/frame in PID \(pid)")
        }
        let axWindow = unsafeBitCast(rawAXWindow, to: AXUIElement.self)
        guard let buttonRect = axRect(button),
              let axWindowRect = axRect(axWindow),
              axWindowRect.insetBy(dx: -2, dy: -2).contains(buttonRect) else {
            throw VisualToolError(message: "unique AXButton \(title.debugDescription) is not within its owning AXWindow in PID \(pid)")
        }
        let orderedWindows = windowRecords()
        let targetWindows = orderedWindows.filter { window in
            guard (window["ownerPID"] as? Int) == Int(pid),
                  let bounds = cgRect(window["bounds"]) else { return false }
            return bounds.insetBy(dx: -2, dy: -2).contains(buttonRect)
        }
        guard targetWindows.count == 1, let targetWindow = targetWindows.first,
              (targetWindow["alpha"] as? Double ?? 0) > 0 else {
            throw VisualToolError(message: "AXButton owning window does not map to exactly one visible CGWindow for PID \(pid)")
        }
        let targetZ = targetWindow["zIndex"] as? Int ?? Int.max
        let frontmost = NSWorkspace.shared.frontmostApplication
        let primaryDisplay = CGDisplayBounds(CGMainDisplayID())
        let ignoreRemoteOverlay = ProcessInfo.processInfo.environment["INCODEX_VISUAL_IGNORE_UU_REMOTE_OVERLAY"] == "1"
        let expectedHostFocus = frontmost?.processIdentifier == pid
        let expectedHelperFocus = frontmost?.bundleIdentifier == "com.apple.systempreferences" &&
            (targetWindow["layer"] as? Int) == 3
        let targetBounds = cgRect(targetWindow["bounds"])
        let expectedPlaceholderFocus = settingsPlaceholderDiagnostic &&
            frontmost?.bundleIdentifier == "com.apple.systempreferences" &&
            (targetWindow["layer"] as? Int) == 0 &&
            targetBounds.map { abs($0.width - 600) <= 2 && (300...400).contains($0.height) } == true
        let ignoredNonInteractiveOverlays = orderedWindows.filter { window in
            guard let ownerName = window["ownerName"] as? String,
                  let layer = window["layer"] as? Int,
                  let bounds = cgRect(window["bounds"]),
                  abs(bounds.minX - primaryDisplay.minX) <= 1,
                  abs(bounds.minY - primaryDisplay.minY) <= 1,
                  abs(bounds.width - primaryDisplay.width) <= 1,
                  abs(bounds.height - primaryDisplay.height) <= 1 else { return false }
            // The full-screen Dock desktop surface is not an input blocker.
            // A smaller Dock menu/window must still fail the occlusion guard.
            if ownerName == "Dock" && layer == 20 { return true }
            return ignoreRemoteOverlay &&
                (expectedHostFocus || expectedHelperFocus || expectedPlaceholderFocus) &&
                ownerName == "UURemoteServer" && layer == 2147483631
        }
        let ignoredNonInteractiveOverlayIDs = Set(ignoredNonInteractiveOverlays.compactMap { $0["windowID"] as? Int })
        let occluders = orderedWindows.filter { window in
            guard (window["zIndex"] as? Int ?? Int.max) < targetZ,
                  (window["alpha"] as? Double ?? 0) > 0,
                  let bounds = cgRect(window["bounds"]) else { return false }
            if let windowID = window["windowID"] as? Int, ignoredNonInteractiveOverlayIDs.contains(windowID) { return false }
            let intersection = bounds.intersection(buttonRect)
            return !intersection.isNull && intersection.width > 0 && intersection.height > 0
        }
        var settingsOccluders: [[String: Any]] = []
        if settingsPlaceholderDiagnostic {
            let ownerBounds = cgRect(targetWindow["bounds"])
            guard (targetWindow["layer"] as? Int) == 0,
                  let ownerBounds,
                  abs(ownerBounds.width - 600) <= 2,
                  (300...400).contains(ownerBounds.height) else {
                throw VisualToolError(message: "Settings placeholder AXButton is not in the production 600-point initial window")
            }
            guard let frontmost,
                  frontmost.bundleIdentifier == "com.apple.systempreferences" else {
                throw VisualToolError(message: "Settings placeholder AXPress requires real System Settings to be frontmost")
            }
            settingsOccluders = occluders.filter {
                ($0["ownerPID"] as? Int) == Int(frontmost.processIdentifier) && ($0["layer"] as? Int) == 0
            }
            guard !settingsOccluders.isEmpty else {
                throw VisualToolError(message: "Settings does not occlude the production placeholder button")
            }
            let unexpectedOccluders = occluders.filter {
                ($0["ownerPID"] as? Int) != Int(frontmost.processIdentifier) || ($0["layer"] as? Int) != 0
            }
            guard unexpectedOccluders.isEmpty else {
                throw VisualToolError(message: "Settings placeholder button has non-Settings occluders: \(unexpectedOccluders.map { $0["windowID"] ?? "?" })")
            }
        } else {
            guard occluders.isEmpty else {
                throw VisualToolError(message: "refusing AXPress: button is covered by higher z-order window(s): \(occluders.map { $0["windowID"] ?? "?" })")
            }
        }
        let frontmostWindow = orderedWindows.first { window in
            guard let frontmostPID = frontmost?.processIdentifier else { return false }
            return (window["ownerPID"] as? Int) == Int(frontmostPID) && (window["alpha"] as? Double ?? 0) > 0
        }
        let action = AXUIElementPerformAction(button, kAXPressAction as CFString)
        guard action.rawValue == 0 else {
            throw VisualToolError(message: "AXPress failed for PID \(pid), \(title.debugDescription): AXError \(action.rawValue)")
        }
        writeJSON([
            "ok": true,
            "command": settingsPlaceholderDiagnostic ? "press-settings-placeholder" : "press",
            "pid": Int(pid),
            "role": "AXButton",
            "buttonAXRole": buttonRole as Any? ?? NSNull(),
            "title": title,
            "buttonAXTitle": buttonTitle as Any? ?? NSNull(),
            "buttonAXDescription": buttonDescription as Any? ?? NSNull(),
            "buttonAXValue": buttonValue as Any? ?? NSNull(),
            "buttonAXEnabled": copyAttribute(button, kAXEnabledAttribute as CFString) as? Bool as Any? ?? NSNull(),
            "buttonAXFocused": copyAttribute(button, kAXFocusedAttribute as CFString) as? Bool as Any? ?? NSNull(),
            "axActionNames": [String](actionNames as? [String] ?? []),
            "axActionNamesError": actionNamesError.rawValue,
            "uniqueMatchCount": buttons.count,
            "axWindowTitle": copyAttribute(axWindow, kAXTitleAttribute as CFString) as? String ?? "",
            "axWindowBounds": rectObject(axWindowRect),
            "buttonBounds": rectObject(buttonRect),
            "ownerCGWindow": targetWindow,
            "frontmostPIDBeforePress": frontmost.map { Int($0.processIdentifier) } as Any? ?? NSNull(),
            "frontmostBundleIDBeforePress": frontmost?.bundleIdentifier as Any? ?? NSNull(),
            "frontmostWindowBeforePress": frontmostWindow as Any? ?? NSNull(),
            "occludingWindows": occluders,
            "settingsOccludingWindows": settingsOccluders,
            "ignoredNonInteractiveOverlays": ignoredNonInteractiveOverlays,
            "axError": action.rawValue,
            "wallTime": ISO8601DateFormatter().string(from: Date()),
            "monotonicSeconds": ProcessInfo.processInfo.systemUptime,
        ])
    }

    private static func reportExactSettingsWindow(_ arguments: [String]) throws {
        guard arguments.count == 3, let pid = pid_t(arguments[1]) else {
            throw VisualToolError(message: "settings-window requires PID EXECUTABLE_PATH")
        }
        let executable = standardizedExecutable(arguments[2])
        let selected = try exactVisibleSettingsWindow(pid: pid, executable: executable)
        var result = settingsWindowJSON(command: "settings-window", selected: selected)
        do {
            result["closeButton"] = try exactSettingsCloseButton(for: selected).json
        } catch {
            result["closeButton"] = ["available": false, "error": error.localizedDescription]
        }
        writeJSON(result)
    }

    private static func moveExactSettingsWindow(_ arguments: [String]) throws {
        guard arguments.count == 8, let pid = pid_t(arguments[1]), let windowID = Int(arguments[3]),
              let expectedBounds = argumentsBounds(arguments, from: 4) else {
            throw VisualToolError(message: "settings-move requires PID EXECUTABLE_PATH WINDOW_ID X Y WIDTH HEIGHT")
        }
        guard AXIsProcessTrusted() else {
            throw VisualToolError(message: "Accessibility control is not already authorized; no prompt was requested")
        }
        let executable = standardizedExecutable(arguments[2])
        let selected = try exactVisibleSettingsWindow(pid: pid, executable: executable)
        try requireExactWindow(selected, windowID: windowID, expectedBounds: expectedBounds)

        let displayBounds = activeDisplayContaining(selected.cgBounds)
        guard let displayBounds else {
            throw VisualToolError(message: "selected Settings window does not fit wholly on one active display; no move was attempted")
        }
        let roomRight = max(0, displayBounds.maxX - selected.cgBounds.maxX)
        let roomLeft = max(0, selected.cgBounds.minX - displayBounds.minX)
        let roomDown = max(0, displayBounds.maxY - selected.cgBounds.maxY)
        let roomUp = max(0, selected.cgBounds.minY - displayBounds.minY)
        let dx = roomRight >= 8 ? min(24, roomRight) : (roomLeft >= 8 ? -min(24, roomLeft) : 0)
        let dy = roomDown >= 8 ? min(16, roomDown) : (roomUp >= 8 ? -min(16, roomUp) : 0)
        guard dx != 0 || dy != 0 else {
            throw VisualToolError(message: "no bounded 8-point Settings window offset fits on the selected display")
        }
        let originalBounds = selected.axBounds
        let requestedPosition = CGPoint(x: originalBounds.minX + dx, y: originalBounds.minY + dy)
        let startedAt = ProcessInfo.processInfo.systemUptime
        let startedWallTime = ISO8601DateFormatter().string(from: Date())
        do {
            try setWindowPosition(selected.element, to: requestedPosition)
            let moved = try waitForWindowPosition(selected.element, expected: requestedPosition, originalSize: originalBounds.size)
            let matchingCG = try waitForCGWindow(windowID: windowID, pid: pid, expected: moved, timeout: 1.0)
            writeJSON([
                "ok": true,
                "command": "settings-move",
                "pid": Int(pid),
                "executablePath": executable,
                "windowID": windowID,
                "title": selected.title,
                "axRole": selected.role,
                "actions": selected.actions,
                "beforeBounds": rectObject(originalBounds),
                "requestedBounds": rectObject(CGRect(origin: requestedPosition, size: originalBounds.size)),
                "afterBounds": rectObject(moved),
                "cgWindowAfterMove": matchingCG,
                "offset": ["x": dx, "y": dy],
                "boundedOffsetMaximum": ["x": 24, "y": 16],
                "wallTimeStarted": startedWallTime,
                "wallTimeFinished": ISO8601DateFormatter().string(from: Date()),
                "monotonicStartedSeconds": startedAt,
                "monotonicFinishedSeconds": ProcessInfo.processInfo.systemUptime,
            ])
        } catch {
            var rollback: [String: Any] = ["attempted": true, "ok": false]
            do {
                try setWindowPosition(selected.element, to: originalBounds.origin)
                let restored = try waitForWindowPosition(selected.element, expected: originalBounds.origin, originalSize: originalBounds.size)
                rollback = ["attempted": true, "ok": true, "bounds": rectObject(restored)]
            } catch {
                rollback["error"] = error.localizedDescription
            }
            throw VisualToolError(message: "Settings move did not verify; rollback=\(rollback), cause=\(error.localizedDescription)")
        }
    }

    private static func restoreExactSettingsWindowPosition(_ arguments: [String]) throws {
        guard arguments.count == 10, let pid = pid_t(arguments[1]), let windowID = Int(arguments[3]),
              let expectedBounds = argumentsBounds(arguments, from: 4),
              let targetX = Double(arguments[8]), let targetY = Double(arguments[9]),
              targetX.isFinite, targetY.isFinite else {
            throw VisualToolError(message: "settings-restore-position requires PID EXECUTABLE_PATH WINDOW_ID X Y WIDTH HEIGHT TARGET_X TARGET_Y")
        }
        guard AXIsProcessTrusted() else {
            throw VisualToolError(message: "Accessibility control is not already authorized; no prompt was requested")
        }
        let executable = standardizedExecutable(arguments[2])
        let selected = try exactVisibleSettingsWindow(pid: pid, executable: executable)
        try requireExactWindow(selected, windowID: windowID, expectedBounds: expectedBounds)
        let target = CGPoint(x: targetX, y: targetY)
        let display = activeDisplayContaining(CGRect(origin: target, size: selected.axBounds.size))
        guard display != nil else {
            throw VisualToolError(message: "original Settings rectangle no longer fits on an active display; no position change was attempted")
        }
        let startedAt = ProcessInfo.processInfo.systemUptime
        let startedWallTime = ISO8601DateFormatter().string(from: Date())
        try setWindowPosition(selected.element, to: target)
        let restored = try waitForWindowPosition(selected.element, expected: target, originalSize: selected.axBounds.size)
        let matchingCG = try waitForCGWindow(windowID: windowID, pid: pid, expected: restored, timeout: 1.0)
        writeJSON([
            "ok": true,
            "command": "settings-restore-position",
            "pid": Int(pid),
            "executablePath": executable,
            "windowID": windowID,
            "title": selected.title,
            "beforeBounds": rectObject(selected.axBounds),
            "targetBounds": rectObject(CGRect(origin: target, size: selected.axBounds.size)),
            "afterBounds": rectObject(restored),
            "cgWindowAfterRestore": matchingCG,
            "wallTimeStarted": startedWallTime,
            "wallTimeFinished": ISO8601DateFormatter().string(from: Date()),
            "monotonicStartedSeconds": startedAt,
            "monotonicFinishedSeconds": ProcessInfo.processInfo.systemUptime,
        ])
    }

    private static func closeExactSettingsWindow(_ arguments: [String], duringFlightForHostPID hostPID: pid_t?) throws {
        let expectedCount = hostPID == nil ? 8 : 9
        guard arguments.count == expectedCount, let pid = pid_t(arguments[1]), let windowID = Int(arguments[3]),
              let expectedBounds = argumentsBounds(arguments, from: 4) else {
            throw VisualToolError(message: hostPID == nil
                ? "settings-close requires PID EXECUTABLE_PATH WINDOW_ID X Y WIDTH HEIGHT"
                : "settings-close-during-flight requires PID EXECUTABLE_PATH WINDOW_ID X Y WIDTH HEIGHT FLIGHT_HOST_PID")
        }
        guard AXIsProcessTrusted() else {
            throw VisualToolError(message: "Accessibility control is not already authorized; no prompt was requested")
        }
        let executable = standardizedExecutable(arguments[2])
        let selected = try exactVisibleSettingsWindow(pid: pid, executable: executable)
        try requireExactWindow(selected, windowID: windowID, expectedBounds: expectedBounds)

        let closeButton = try exactSettingsCloseButton(for: selected)
        let flightWindows: [[String: Any]]
        if let hostPID {
            flightWindows = windowRecords().filter { window in
                (window["ownerPID"] as? Int) == Int(hostPID) &&
                    (window["layer"] as? Int) == 25 &&
                    (window["alpha"] as? Double ?? 0) > 0
            }
            guard !flightWindows.isEmpty else {
                throw VisualToolError(message: "reverse-flight layer-25 host window is no longer visible; Settings close-button AXPress was not performed")
            }
        } else {
            flightWindows = []
        }
        let startedAt = ProcessInfo.processInfo.systemUptime
        let startedWallTime = ISO8601DateFormatter().string(from: Date())
        let action = AXUIElementPerformAction(closeButton.element, kAXPressAction as CFString)
        guard action.rawValue == 0 else {
            throw VisualToolError(message: "AXPress on the exact Settings close button failed for PID \(pid) window \(windowID): AXError \(action.rawValue)")
        }
        let deadline = Date().addingTimeInterval(1.5)
        var remainsVisible = true
        while Date() < deadline {
            remainsVisible = windowRecords().contains { window in
                (window["ownerPID"] as? Int) == Int(pid) &&
                    (window["windowID"] as? Int) == windowID &&
                    (window["layer"] as? Int) == 0 &&
                    (window["alpha"] as? Double ?? 0) > 0
            }
            if !remainsVisible { break }
            Thread.sleep(forTimeInterval: 0.04)
        }
        guard !remainsVisible else {
            throw VisualToolError(message: "close-button AXPress returned success but exact Settings window \(windowID) remained visible")
        }
        writeJSON([
            "ok": true,
            "command": hostPID == nil ? "settings-close" : "settings-close-during-flight",
            "pid": Int(pid),
            "executablePath": executable,
            "windowID": windowID,
            "title": selected.title,
            "axRole": selected.role,
            "windowActions": selected.actions,
            "closeButton": closeButton.json,
            "performedAction": kAXPressAction as String,
            "boundsAtClose": rectObject(selected.axBounds),
            "flightHostPID": hostPID.map { Int($0) } as Any? ?? NSNull(),
            "flightWindowsVisibleAtButtonPress": flightWindows,
            "axError": action.rawValue,
            "windowAbsentAfterClose": true,
            "wallTimeStarted": startedWallTime,
            "wallTimeFinished": ISO8601DateFormatter().string(from: Date()),
            "monotonicStartedSeconds": startedAt,
            "monotonicFinishedSeconds": ProcessInfo.processInfo.systemUptime,
        ])
    }

    private struct SelectedSettingsWindow {
        let element: AXUIElement
        let pid: pid_t
        let windowID: Int
        let title: String
        let role: String
        let actions: [String]
        let axBounds: CGRect
        let cgBounds: CGRect
        let cgRecord: [String: Any]
    }

    private struct ExactSettingsCloseButton {
        let element: AXUIElement
        let role: String
        let subrole: String
        let actions: [String]
        let bounds: CGRect
        let label: String
        let ownerPID: pid_t
        let ownerWindowID: Int
        let ownerWindowTitle: String

        var json: [String: Any] {
            [
                "available": true,
                "role": role,
                "subrole": subrole,
                "actions": actions,
                "bounds": rectObject(bounds),
                "label": label,
                "ownerPID": Int(ownerPID),
                "ownerWindowID": ownerWindowID,
                "ownerWindowTitle": ownerWindowTitle,
                "ownerMatchesSelectedAXWindow": true,
            ]
        }
    }

    private static func exactSettingsCloseButton(for selected: SelectedSettingsWindow) throws -> ExactSettingsCloseButton {
        guard let rawButton = copyAttribute(selected.element, kAXCloseButtonAttribute as CFString),
              CFGetTypeID(rawButton) == AXUIElementGetTypeID() else {
            throw VisualToolError(message: "exact Settings AXWindow has no accessible AXCloseButton; no close was performed")
        }
        let button = unsafeBitCast(rawButton, to: AXUIElement.self)
        let role = copyAttribute(button, kAXRoleAttribute as CFString) as? String ?? ""
        let subrole = copyAttribute(button, kAXSubroleAttribute as CFString) as? String ?? ""
        guard role == (kAXButtonRole as String), subrole == (kAXCloseButtonSubrole as String) else {
            throw VisualToolError(message: "Settings AXCloseButton did not identify as AXButton/AXCloseButton; no close was performed")
        }
        guard copyAttribute(button, kAXEnabledAttribute as CFString) as? Bool == true,
              copyAttribute(button, kAXHiddenAttribute as CFString) as? Bool != true else {
            throw VisualToolError(message: "Settings AXCloseButton is disabled or hidden; no close was performed")
        }
        guard let rawOwner = copyAttribute(button, kAXWindowAttribute as CFString),
              CFGetTypeID(rawOwner) == AXUIElementGetTypeID(), CFEqual(rawOwner, selected.element) else {
            throw VisualToolError(message: "Settings close button is not owned by the exact selected AXWindow; no close was performed")
        }
        var rawActions: CFArray?
        _ = AXUIElementCopyActionNames(button, &rawActions)
        let actions = [String](rawActions as? [String] ?? [])
        guard actions.contains(kAXPressAction as String) else {
            throw VisualToolError(message: "Settings AXCloseButton does not expose AXPress; no close was performed")
        }
        guard let bounds = axRect(button),
              selected.axBounds.insetBy(dx: -2, dy: -2).contains(bounds),
              bounds.width <= 64, bounds.height <= 64,
              bounds.minX <= selected.axBounds.minX + 90,
              bounds.minY <= selected.axBounds.minY + 90 else {
            throw VisualToolError(message: "Settings AXCloseButton is outside the expected top-left close-control frame; no close was performed")
        }
        return ExactSettingsCloseButton(
            element: button,
            role: role,
            subrole: subrole,
            actions: actions,
            bounds: bounds,
            label: copyAttribute(button, kAXTitleAttribute as CFString) as? String ?? "",
            ownerPID: selected.pid,
            ownerWindowID: selected.windowID,
            ownerWindowTitle: selected.title
        )
    }

    private static func exactVisibleSettingsWindow(pid: pid_t, executable: String) throws -> SelectedSettingsWindow {
        guard let running = NSRunningApplication(processIdentifier: pid),
              running.bundleIdentifier == "com.apple.systempreferences",
              running.executableURL?.standardizedFileURL.path == executable else {
            throw VisualToolError(message: "Settings PID does not match the exact System Settings bundle and executable")
        }
        let appElement = AXUIElementCreateApplication(pid)
        let appWindows = copyAttribute(appElement, kAXWindowsAttribute as CFString) as? [AXUIElement] ?? []
        let visibleMainCGWindows = windowRecords().filter { window in
            guard (window["ownerPID"] as? Int) == Int(pid),
                  (window["layer"] as? Int) == 0,
                  (window["alpha"] as? Double ?? 0) > 0,
                  let bounds = cgRect(window["bounds"]) else { return false }
            return bounds.width > 600 && bounds.height >= 470
        }
        guard visibleMainCGWindows.count == 1, let cgRecord = visibleMainCGWindows.first,
              let windowID = cgRecord["windowID"] as? Int,
              let cgBounds = cgRect(cgRecord["bounds"]) else {
            throw VisualToolError(message: "expected exactly one visible primary System Settings window for PID \(pid); found \(visibleMainCGWindows.count)")
        }
        let matches = appWindows.compactMap { window -> SelectedSettingsWindow? in
            let role = copyAttribute(window, kAXRoleAttribute as CFString) as? String ?? ""
            guard role == (kAXWindowRole as String),
                  copyAttribute(window, kAXMinimizedAttribute as CFString) as? Bool != true,
                  let bounds = axRect(window), rectsMatch(bounds, cgBounds, tolerance: 2) else { return nil }
            var rawActions: CFArray?
            _ = AXUIElementCopyActionNames(window, &rawActions)
            return SelectedSettingsWindow(
                element: window,
                pid: pid,
                windowID: windowID,
                title: copyAttribute(window, kAXTitleAttribute as CFString) as? String ?? "",
                role: role,
                actions: [String](rawActions as? [String] ?? []),
                axBounds: bounds,
                cgBounds: cgBounds,
                cgRecord: cgRecord
            )
        }
        guard matches.count == 1, let selected = matches.first else {
            throw VisualToolError(message: "visible Settings CGWindow did not map to exactly one AXWindow for PID \(pid); matches=\(matches.count)")
        }
        return selected
    }

    private static func settingsWindowJSON(command: String, selected: SelectedSettingsWindow) -> [String: Any] {
        [
            "ok": true,
            "command": command,
            "pid": Int(selected.pid),
            "windowID": selected.windowID,
            "title": selected.title,
            "axRole": selected.role,
            "actions": selected.actions,
            "bounds": rectObject(selected.axBounds),
            "cgWindow": selected.cgRecord,
            "wallTime": ISO8601DateFormatter().string(from: Date()),
            "monotonicSeconds": ProcessInfo.processInfo.systemUptime,
        ]
    }

    private static func requireExactWindow(_ selected: SelectedSettingsWindow, windowID: Int, expectedBounds: CGRect) throws {
        guard selected.windowID == windowID, rectsMatch(selected.axBounds, expectedBounds, tolerance: 1),
              rectsMatch(selected.cgBounds, expectedBounds, tolerance: 2) else {
            throw VisualToolError(message: "refusing to mutate Settings: exact window ID/frame mismatch; actualID=\(selected.windowID), expectedID=\(windowID), actualAX=\(selected.axBounds), actualCG=\(selected.cgBounds), expected=\(expectedBounds)")
        }
    }

    private static func standardizedExecutable(_ path: String) -> String {
        URL(fileURLWithPath: path).standardizedFileURL.path
    }

    private static func argumentsBounds(_ arguments: [String], from start: Int) -> CGRect? {
        guard arguments.count >= start + 4,
              let x = Double(arguments[start]), let y = Double(arguments[start + 1]),
              let width = Double(arguments[start + 2]), let height = Double(arguments[start + 3]),
              x.isFinite, y.isFinite, width.isFinite, height.isFinite,
              width > 0, height > 0 else { return nil }
        return CGRect(x: x, y: y, width: width, height: height)
    }

    private static func rectsMatch(_ lhs: CGRect, _ rhs: CGRect, tolerance: CGFloat) -> Bool {
        abs(lhs.minX - rhs.minX) <= tolerance && abs(lhs.minY - rhs.minY) <= tolerance &&
            abs(lhs.width - rhs.width) <= tolerance && abs(lhs.height - rhs.height) <= tolerance
    }

    private static func activeDisplayContaining(_ rect: CGRect) -> CGRect? {
        var displayCount: UInt32 = 0
        guard CGGetActiveDisplayList(0, nil, &displayCount) == .success, displayCount > 0 else { return nil }
        var displayIDs = Array(repeating: CGDirectDisplayID(), count: Int(displayCount))
        guard CGGetActiveDisplayList(displayCount, &displayIDs, &displayCount) == .success else { return nil }
        return displayIDs.map(CGDisplayBounds).first { $0.insetBy(dx: -1, dy: -1).contains(rect) }
    }

    private static func setWindowPosition(_ window: AXUIElement, to point: CGPoint) throws {
        var valuePoint = point
        guard let value = AXValueCreate(.cgPoint, &valuePoint) else {
            throw VisualToolError(message: "could not create an AX position value")
        }
        let result = AXUIElementSetAttributeValue(window, kAXPositionAttribute as CFString, value)
        guard result.rawValue == 0 else {
            throw VisualToolError(message: "AX position update failed: AXError \(result.rawValue)")
        }
    }

    private static func waitForWindowPosition(_ window: AXUIElement, expected: CGPoint, originalSize: CGSize) throws -> CGRect {
        let deadline = Date().addingTimeInterval(1.0)
        var latest = axRect(window)
        while Date() < deadline {
            if let frame = axRect(window) {
                latest = frame
                if abs(frame.minX - expected.x) <= 1 && abs(frame.minY - expected.y) <= 1 &&
                    abs(frame.width - originalSize.width) <= 1 && abs(frame.height - originalSize.height) <= 1 {
                    return frame
                }
            }
            Thread.sleep(forTimeInterval: 0.025)
        }
        throw VisualToolError(message: "Settings AXWindow did not reach requested position while preserving size; expected=(\(expected.x),\(expected.y),\(originalSize.width),\(originalSize.height)) actual=\(String(describing: latest))")
    }

    private static func waitForCGWindow(windowID: Int, pid: pid_t, expected: CGRect, timeout: TimeInterval) throws -> [String: Any] {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if let match = windowRecords().first(where: {
                ($0["ownerPID"] as? Int) == Int(pid) && ($0["windowID"] as? Int) == windowID
            }), let bounds = cgRect(match["bounds"]), rectsMatch(bounds, expected, tolerance: 2) {
                return match
            }
            Thread.sleep(forTimeInterval: 0.025)
        }
        throw VisualToolError(message: "CGWindow \(windowID) for Settings PID \(pid) did not confirm AX geometry \(expected)")
    }

    @MainActor
    private static func activateInitialAllowByKeyboard(_ arguments: [String]) throws {
        guard arguments.count == 5, let pid = pid_t(arguments[1]) else {
            throw VisualToolError(message: "keyboard-allow requires PID ALLOW_LABEL SKIP_LABEL INITIAL_WINDOW_TITLE")
        }
        guard AXIsProcessTrusted() else {
            throw VisualToolError(message: "Accessibility control is not already authorized; no prompt was requested")
        }
        guard CGPreflightPostEventAccess() else {
            throw VisualToolError(message: "CGPreflightPostEventAccess=false; Return was not sent and no permission prompt was requested")
        }
        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == pid else {
            throw VisualToolError(message: "candidate host is not frontmost; Return was not sent")
        }

        let allowLabel = arguments[2]
        let skipLabel = arguments[3]
        let expectedWindowTitle = arguments[4]
        let appElement = AXUIElementCreateApplication(pid)
        let windows = copyAttribute(appElement, kAXWindowsAttribute as CFString) as? [AXUIElement] ?? []
        var allowButtons: [AXUIElement] = []
        var skipButtons: [AXUIElement] = []
        var visitCount = 0
        for window in windows {
            collectButtons(window, expectedTitle: allowLabel, depth: 0, visitCount: &visitCount, into: &allowButtons)
            collectButtons(window, expectedTitle: skipLabel, depth: 0, visitCount: &visitCount, into: &skipButtons)
        }
        guard allowButtons.count == 1, let allow = allowButtons.first,
              skipButtons.count == 1, let skip = skipButtons.first else {
            throw VisualToolError(message: "expected exactly one initial Allow and Skip AXButton; found Allow=\(allowButtons.count), Skip=\(skipButtons.count); Return was not sent")
        }
        guard let focusedWindow = axElement(appElement, kAXFocusedWindowAttribute as CFString) else {
            throw VisualToolError(message: "candidate host has no AXFocusedWindow; Return was not sent")
        }
        let focusedWindowTitle = copyAttribute(focusedWindow, kAXTitleAttribute as CFString) as? String ?? ""
        guard focusedWindowTitle == expectedWindowTitle else {
            throw VisualToolError(message: "focused window title did not match the candidate initial page; Return was not sent")
        }

        let allowSemantics = try initialButtonSemantics(allow, expectedLabel: allowLabel, expectedWindowTitle: expectedWindowTitle)
        let skipSemantics = try initialButtonSemantics(skip, expectedLabel: skipLabel, expectedWindowTitle: expectedWindowTitle)
        let applicationFocusBefore = axElement(appElement, kAXFocusedUIElementAttribute as CFString)
        let systemFocusBefore = axElement(AXUIElementCreateSystemWide(), kAXFocusedUIElementAttribute as CFString)
        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == pid,
              CGPreflightPostEventAccess() else {
            throw VisualToolError(message: "candidate host focus or post-event access changed before Return; no key was sent")
        }
        guard let down = CGEvent(keyboardEventSource: nil, virtualKey: 36, keyDown: true),
              let up = CGEvent(keyboardEventSource: nil, virtualKey: 36, keyDown: false) else {
            throw VisualToolError(message: "could not construct the Return key event; no key was sent")
        }

        // This is the only initial Allow activation in the diagnostic path.
        // Never fall back to AXPress if the event is unavailable or fails.
        down.post(tap: .cghidEventTap)
        up.post(tap: .cghidEventTap)
        Thread.sleep(forTimeInterval: 0.12)

        let applicationFocusAfter = axElement(appElement, kAXFocusedUIElementAttribute as CFString)
        let systemFocusAfter = axElement(AXUIElementCreateSystemWide(), kAXFocusedUIElementAttribute as CFString)
        writeJSON([
            "ok": true,
            "command": "keyboard-allow",
            "targetPID": Int(pid),
            "expectedWindowTitle": expectedWindowTitle,
            "focusedWindowTitleBefore": focusedWindowTitle,
            "frontmostPIDBefore": Int(pid),
            "frontmostPIDAfter": NSWorkspace.shared.frontmostApplication.map { Int($0.processIdentifier) } as Any? ?? NSNull(),
            "allowAXRole": allowSemantics["role"] ?? NSNull(),
            "allowAXLabel": allowSemantics["label"] ?? NSNull(),
            "allowAXEnabled": allowSemantics["enabled"] ?? NSNull(),
            "allowAXFocused": allowSemantics["focused"] ?? NSNull(),
            "allowAXTitle": allowSemantics["title"] ?? NSNull(),
            "allowAXDescription": allowSemantics["description"] ?? NSNull(),
            "allowAXValue": allowSemantics["value"] ?? NSNull(),
            "allowAXActionNames": allowSemantics["actions"] ?? [],
            "skipAXRole": skipSemantics["role"] ?? NSNull(),
            "skipAXLabel": skipSemantics["label"] ?? NSNull(),
            "skipAXEnabled": skipSemantics["enabled"] ?? NSNull(),
            "skipAXFocused": skipSemantics["focused"] ?? NSNull(),
            "skipAXTitle": skipSemantics["title"] ?? NSNull(),
            "skipAXDescription": skipSemantics["description"] ?? NSNull(),
            "skipAXValue": skipSemantics["value"] ?? NSNull(),
            "skipAXActionNames": skipSemantics["actions"] ?? [],
            "applicationFocusedElementBefore": focusSummary(applicationFocusBefore),
            "systemFocusedElementBefore": focusSummary(systemFocusBefore),
            "applicationFocusedElementAfter": focusSummary(applicationFocusAfter),
            "systemFocusedElementAfter": focusSummary(systemFocusAfter),
            "keyboardKey": "Return",
            "logicalKeyboardPresses": 1,
            "keyboardEventsPosted": 2,
            "allowAXPressPerformed": false,
            "postEventAccessPreflight": true,
            "wallTime": ISO8601DateFormatter().string(from: Date()),
            "monotonicSeconds": ProcessInfo.processInfo.systemUptime,
        ])
    }

    private static func initialButtonSemantics(
        _ button: AXUIElement,
        expectedLabel: String,
        expectedWindowTitle: String,
    ) throws -> [String: Any] {
        let role = copyAttribute(button, kAXRoleAttribute as CFString) as? String
        guard role == (kAXButtonRole as String) else {
            throw VisualToolError(message: "initial control \(expectedLabel.debugDescription) is not an AXButton; Return was not sent")
        }
        guard buttonTitles(button).contains(expectedLabel) else {
            throw VisualToolError(message: "initial control AX label did not match \(expectedLabel.debugDescription); Return was not sent")
        }
        guard copyAttribute(button, kAXEnabledAttribute as CFString) as? Bool == true,
              copyAttribute(button, kAXHiddenAttribute as CFString) as? Bool != true else {
            throw VisualToolError(message: "initial control \(expectedLabel.debugDescription) is disabled or hidden; Return was not sent")
        }
        guard let rawWindow = copyAttribute(button, kAXWindowAttribute as CFString),
              CFGetTypeID(rawWindow) == AXUIElementGetTypeID() else {
            throw VisualToolError(message: "initial control \(expectedLabel.debugDescription) has no AXWindow; Return was not sent")
        }
        let window = unsafeBitCast(rawWindow, to: AXUIElement.self)
        let windowTitle = copyAttribute(window, kAXTitleAttribute as CFString) as? String ?? ""
        guard windowTitle == expectedWindowTitle else {
            throw VisualToolError(message: "initial control \(expectedLabel.debugDescription) belongs to an unexpected AXWindow; Return was not sent")
        }
        var actions: CFArray?
        let actionResult = AXUIElementCopyActionNames(button, &actions)
        let actionNames = [String](actions as? [String] ?? [])
        guard actionNames.contains(kAXPressAction as String) else {
            throw VisualToolError(message: "initial control \(expectedLabel.debugDescription) does not expose AXPress; Return was not sent")
        }
        return [
            "role": role as Any,
            "label": expectedLabel,
            "enabled": true,
            "focused": copyAttribute(button, kAXFocusedAttribute as CFString) as? Bool as Any? ?? NSNull(),
            "title": copyAttribute(button, kAXTitleAttribute as CFString) as? String as Any? ?? NSNull(),
            "description": copyAttribute(button, kAXDescriptionAttribute as CFString) as? String as Any? ?? NSNull(),
            "value": copyAttribute(button, kAXValueAttribute as CFString).map { String(describing: $0) } as Any? ?? NSNull(),
            "actions": actionNames,
            "actionsResult": actionResult.rawValue,
        ]
    }

    private static func axElement(_ element: AXUIElement, _ name: CFString) -> AXUIElement? {
        guard let value = copyAttribute(element, name), CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
        return unsafeBitCast(value, to: AXUIElement.self)
    }

    private static func focusSummary(_ element: AXUIElement?) -> [String: Any] {
        guard let element else { return ["available": false] }
        return [
            "available": true,
            "role": copyAttribute(element, kAXRoleAttribute as CFString) as? String as Any? ?? NSNull(),
            "title": copyAttribute(element, kAXTitleAttribute as CFString) as? String as Any? ?? NSNull(),
            "description": copyAttribute(element, kAXDescriptionAttribute as CFString) as? String as Any? ?? NSNull(),
            "value": copyAttribute(element, kAXValueAttribute as CFString) as? String as Any? ?? NSNull(),
            "focused": copyAttribute(element, kAXFocusedAttribute as CFString) as? Bool as Any? ?? NSNull(),
        ]
    }

    private static func copyAttribute(_ element: AXUIElement, _ name: CFString) -> CFTypeRef? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(element, name, &value).rawValue == 0 else { return nil }
        return value
    }

    private static func captureSnapshot(_ outputURL: URL) async throws -> [String: Any] {
        guard CGPreflightScreenCaptureAccess() else {
            throw VisualToolError(message: "Screen Recording is not already authorized; no request was made")
        }
        let beganAt = ProcessInfo.processInfo.systemUptime
        let beganWallTime = ISO8601DateFormatter().string(from: Date())
        guard #available(macOS 14.0, *) else {
            throw VisualToolError(message: "ScreenCaptureKit screenshot capture requires macOS 14 or later")
        }
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        let display = content.displays.first(where: { $0.displayID == CGMainDisplayID() }) ?? content.displays.first
        guard let display else { throw VisualToolError(message: "ScreenCaptureKit found no shareable display") }
        let filter = SCContentFilter(display: display, excludingApplications: [], exceptingWindows: [])
        let configuration = SCStreamConfiguration()
        configuration.width = Int(display.width)
        configuration.height = Int(display.height)
        configuration.showsCursor = true
        let image = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: configuration)
        let bitmap = NSBitmapImageRep(cgImage: image)
        guard let png = bitmap.representation(using: .png, properties: [:]) else {
            throw VisualToolError(message: "ScreenCaptureKit image could not be encoded as PNG")
        }
        try png.write(to: outputURL, options: .atomic)
        return [
            "ok": true,
            "command": "snapshot",
            "path": outputURL.path,
            "displayID": display.displayID,
            "pixelWidth": image.width,
            "pixelHeight": image.height,
            "windows": windowRecords(),
            "frontmostPID": NSWorkspace.shared.frontmostApplication.map { Int($0.processIdentifier) } as Any? ?? NSNull(),
            "wallTimeStarted": beganWallTime,
            "wallTimeFinished": ISO8601DateFormatter().string(from: Date()),
            "monotonicStartedSeconds": beganAt,
            "monotonicFinishedSeconds": ProcessInfo.processInfo.systemUptime,
        ]
    }

    private static func collectButtons(
        _ element: AXUIElement,
        expectedTitle: String,
        depth: Int,
        visitCount: inout Int,
        into result: inout [AXUIElement],
    ) {
        guard depth <= 32, visitCount < 10_000 else { return }
        visitCount += 1
        let role = copyAttribute(element, kAXRoleAttribute as CFString) as? String
        if role == (kAXButtonRole as String), buttonTitles(element).contains(expectedTitle) {
            result.append(element)
        }
        guard let children = copyAttribute(element, kAXChildrenAttribute as CFString) as? [AXUIElement] else { return }
        for child in children {
            collectButtons(child, expectedTitle: expectedTitle, depth: depth + 1, visitCount: &visitCount, into: &result)
        }
    }

    private static func buttonTitles(_ element: AXUIElement) -> [String] {
        [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute].compactMap { key in
            guard let value = copyAttribute(element, key as CFString) else { return nil }
            if let string = value as? String { return string }
            if let number = value as? NSNumber { return number.stringValue }
            return nil
        }
    }

    private static func axRect(_ element: AXUIElement) -> CGRect? {
        guard let rawPosition = copyAttribute(element, kAXPositionAttribute as CFString),
              let rawSize = copyAttribute(element, kAXSizeAttribute as CFString),
              CFGetTypeID(rawPosition) == AXValueGetTypeID(),
              CFGetTypeID(rawSize) == AXValueGetTypeID() else { return nil }
        let position = unsafeBitCast(rawPosition, to: AXValue.self)
        let size = unsafeBitCast(rawSize, to: AXValue.self)
        var origin = CGPoint.zero
        var extent = CGSize.zero
        guard AXValueGetValue(position, .cgPoint, &origin),
              AXValueGetValue(size, .cgSize, &extent),
              origin.x.isFinite, origin.y.isFinite,
              extent.width.isFinite, extent.height.isFinite,
              extent.width > 0, extent.height > 0 else { return nil }
        return CGRect(origin: origin, size: extent)
    }

    private static func cgRect(_ value: Any?) -> CGRect? {
        guard let bounds = value as? [String: Double],
              let x = bounds["x"], let y = bounds["y"],
              let width = bounds["width"], let height = bounds["height"],
              x.isFinite, y.isFinite, width.isFinite, height.isFinite else { return nil }
        return CGRect(x: x, y: y, width: width, height: height)
    }

    private static func rectObject(_ rect: CGRect) -> [String: Double] {
        ["x": rect.minX, "y": rect.minY, "width": rect.width, "height": rect.height]
    }

    private static func windowRecords() -> [[String: Any]] {
        guard let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else { return [] }
        return list.enumerated().compactMap { index, window in
            guard let number = window[kCGWindowNumber as String] as? NSNumber,
                  let owner = window[kCGWindowOwnerPID as String] as? NSNumber,
                  let layer = window[kCGWindowLayer as String] as? NSNumber,
                  let bounds = window[kCGWindowBounds as String] as? [String: Any] else { return nil }
            let x = (bounds["X"] as? NSNumber)?.doubleValue ?? 0
            let y = (bounds["Y"] as? NSNumber)?.doubleValue ?? 0
            let width = (bounds["Width"] as? NSNumber)?.doubleValue ?? 0
            let height = (bounds["Height"] as? NSNumber)?.doubleValue ?? 0
            return [
                "windowID": number.intValue,
                "ownerPID": owner.intValue,
                "ownerName": window[kCGWindowOwnerName as String] as? String ?? "",
                "title": window[kCGWindowName as String] as? String ?? "",
                "layer": layer.intValue,
                "zIndex": index,
                "alpha": (window[kCGWindowAlpha as String] as? NSNumber)?.doubleValue ?? 0,
                "bounds": ["x": x, "y": y, "width": width, "height": height],
            ]
        }
    }

    private static func writeJSON(_ value: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys, .fragmentsAllowed]) else {
            FileHandle.standardError.write(Data("visual helper could not encode JSON\n".utf8))
            return
        }
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data([10]))
    }
}
