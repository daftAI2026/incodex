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
            throw VisualToolError(message: "expected preflight, app, windows, press, press-settings-placeholder, or snapshot")
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
        case "press":
            try pressButton(arguments, settingsPlaceholderDiagnostic: false)
        case "press-settings-placeholder":
            try pressButton(arguments, settingsPlaceholderDiagnostic: true)
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
        let ignoredRemoteOverlays = orderedWindows.filter { window in
            guard ignoreRemoteOverlay,
                  (expectedHostFocus || expectedHelperFocus || expectedPlaceholderFocus),
                  let ownerName = window["ownerName"] as? String,
                  ownerName == "UURemoteServer",
                  let layer = window["layer"] as? Int,
                  layer == 2147483631,
                  let bounds = cgRect(window["bounds"]),
                  abs(bounds.minX - primaryDisplay.minX) <= 1,
                  abs(bounds.minY - primaryDisplay.minY) <= 1,
                  abs(bounds.width - primaryDisplay.width) <= 1,
                  abs(bounds.height - primaryDisplay.height) <= 1 else { return false }
            return true
        }
        let ignoredRemoteOverlayIDs = Set(ignoredRemoteOverlays.compactMap { $0["windowID"] as? Int })
        let occluders = orderedWindows.filter { window in
            guard (window["zIndex"] as? Int ?? Int.max) < targetZ,
                  (window["alpha"] as? Double ?? 0) > 0,
                  let bounds = cgRect(window["bounds"]) else { return false }
            if let windowID = window["windowID"] as? Int, ignoredRemoteOverlayIDs.contains(windowID) { return false }
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
            "ignoredRemoteOverlays": ignoredRemoteOverlays,
            "axError": action.rawValue,
            "wallTime": ISO8601DateFormatter().string(from: Date()),
            "monotonicSeconds": ProcessInfo.processInfo.systemUptime,
        ])
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
