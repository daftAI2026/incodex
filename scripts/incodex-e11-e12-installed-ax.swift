import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

private struct ToolError: Error, LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

private struct Bounds: Equatable {
    let x: Double
    let y: Double
    let width: Double
    let height: Double

    init(_ rect: CGRect) {
        x = Double(rect.origin.x)
        y = Double(rect.origin.y)
        width = Double(rect.size.width)
        height = Double(rect.size.height)
    }

    init(x: Double, y: Double, width: Double, height: Double) {
        self.x = x
        self.y = y
        self.width = width
        self.height = height
    }

    var json: [String: Double] {
        ["x": x, "y": y, "width": width, "height": height]
    }

    func distance(to other: Bounds) -> Double {
        abs(x - other.x) + abs(y - other.y) + abs(width - other.width) + abs(height - other.height)
    }
}

private struct WindowRecord {
    let element: AXUIElement
    let windowId: Int
    let bounds: Bounds
    let layer: Int
    let alpha: Double
    let zIndex: Int
    let isMain: Bool
    let isMinimized: Bool
    let actions: [String]

    var json: [String: Any] {
        [
            "windowId": windowId,
            "bounds": bounds.json,
            "layer": layer,
            "alpha": alpha,
            "zIndex": zIndex,
            "isMain": isMain,
            "isMinimized": isMinimized,
            "actions": actions,
        ]
    }
}

private struct ToggleRecord {
    let element: AXUIElement
    let role: String
    let label: String
    let kind: String
    let enabled: Bool
    let value: Any
    let bounds: Bounds
    let windowId: Int
    let windowBounds: Bounds
    let actions: [String]

    var json: [String: Any] {
        [
            "role": role,
            "label": label,
            "kind": kind,
            "enabled": enabled,
            "value": value,
            "bounds": bounds.json,
            "windowId": windowId,
            "windowBounds": windowBounds.json,
            "actions": actions,
        ]
    }
}

private struct SearchButtonRecord: Equatable {
    let role: String
    let label: String
    let bounds: Bounds
    let windowId: Int

    var json: [String: Any] {
        ["role": role, "label": label, "bounds": bounds.json, "windowId": windowId]
    }
}

private struct WindowIdentity {
    let windowId: Int
    let layer: Int
    let alpha: Double
    let isMain: Bool
    let isMinimized: Bool

    init(windowId: Int, layer: Int, alpha: Double, isMain: Bool, isMinimized: Bool) {
        self.windowId = windowId
        self.layer = layer
        self.alpha = alpha
        self.isMain = isMain
        self.isMinimized = isMinimized
    }

    init(_ window: WindowRecord) {
        windowId = window.windowId
        layer = window.layer
        alpha = window.alpha
        isMain = window.isMain
        isMinimized = window.isMinimized
    }
}

private enum Helper {
    private static let bundleIdentifier = "com.openai.codex"
    private static let maximumAXNodes = 20_000
    private static let maximumAXDepth = 48

    static func main() {
        do {
            let reply = try dispatch(Array(CommandLine.arguments.dropFirst()))
            try writeJSON(reply)
        } catch {
            do { try writeJSON(["ok": false, "error": error.localizedDescription]) }
            catch { fputs("{\"ok\":false,\"error\":\"failed to encode helper error\"}\n", stderr) }
            Darwin.exit(2)
        }
    }

    private static func dispatch(_ arguments: [String]) throws -> [String: Any] {
        guard let command = arguments.first else { throw ToolError(message: "expected processes, frontmost, self-test, activate, inspect, press, or close-window") }
        let values = Array(arguments.dropFirst())
        switch command {
        case "processes":
            guard values.count == 1 else { throw ToolError(message: "processes requires EXECUTABLE_PATH") }
            let path = standardized(values[0])
            let pids = matchingApplications(executablePath: path).map { Int($0.processIdentifier) }.sorted()
            return ["ok": true, "matchingPids": pids]
        case "frontmost":
            let app = NSWorkspace.shared.frontmostApplication
            let bundle = app?.bundleIdentifier
            let name = app?.localizedName
            let dialogs = visibleSystemDialogs()
            return [
                "ok": true,
                "frontmostPid": app.map { Int($0.processIdentifier) } as Any? ?? NSNull(),
                "frontmostBundleId": bundle as Any? ?? NSNull(),
                "securityAgentActive": isSecurityAgent(bundleIdentifier: bundle, name: name),
                "systemSettingsActive": isSystemSettings(bundleIdentifier: bundle, name: name),
                "securityAgentWindowVisible": dialogs.securityAgent,
                "systemSettingsWindowVisible": dialogs.systemSettings,
            ]
        case "self-test":
            let normal = safeModalBlocker(securityAgentVisible: false, settingsVisible: false, securityAgentFrontmost: false, settingsFrontmost: false)
            let visibleAgent = safeModalBlocker(securityAgentVisible: true, settingsVisible: false, securityAgentFrontmost: false, settingsFrontmost: false)
            let foregroundAgent = safeModalBlocker(securityAgentVisible: false, settingsVisible: false, securityAgentFrontmost: true, settingsFrontmost: false)
            let foregroundSettings = safeModalBlocker(securityAgentVisible: false, settingsVisible: false, securityAgentFrontmost: false, settingsFrontmost: true)
            let backgroundSettings = safeModalBlocker(securityAgentVisible: false, settingsVisible: true, securityAgentFrontmost: false, settingsFrontmost: false)
            guard normal == nil, visibleAgent != nil, foregroundAgent != nil, foregroundSettings != nil, backgroundSettings != nil else {
                throw ToolError(message: "pure system-dialog guard test failed")
            }
            guard exactCGWindowMappingIsUnambiguous(1), !exactCGWindowMappingIsUnambiguous(0), !exactCGWindowMappingIsUnambiguous(2) else {
                throw ToolError(message: "pure AX-to-CG window mapping guard test failed")
            }
            let selectedWindow = selectedMainWindowId([
                WindowIdentity(windowId: 101, layer: 0, alpha: 1, isMain: false, isMinimized: false),
                WindowIdentity(windowId: 202, layer: 0, alpha: 1, isMain: true, isMinimized: false),
            ])
            let ambiguousMain = selectedMainWindowId([
                WindowIdentity(windowId: 101, layer: 0, alpha: 1, isMain: true, isMinimized: false),
                WindowIdentity(windowId: 202, layer: 0, alpha: 1, isMain: true, isMinimized: false),
            ])
            guard selectedWindow == 202, ambiguousMain == nil else {
                throw ToolError(message: "pure exact main-window selection test failed")
            }
            let oldHatSize = Bounds(x: 1, y: 2, width: 24, height: 24)
            let currentHatSize = Bounds(x: 3, y: 4, width: 28, height: 28)
            let wrongHatSize = Bounds(x: 5, y: 6, width: 24, height: 28)
            guard sizesMatchLiveSearch(oldHatSize, oldHatSize),
                  sizesMatchLiveSearch(currentHatSize, currentHatSize),
                  !sizesMatchLiveSearch(currentHatSize, oldHatSize),
                  !sizesMatchLiveSearch(wrongHatSize, currentHatSize) else {
                throw ToolError(message: "pure live Search sizing test failed")
            }
            let search = SearchButtonRecord(role: kAXButtonRole as String, label: "Search", bounds: currentHatSize, windowId: 99)
            let otherWindowSearch = SearchButtonRecord(role: kAXButtonRole as String, label: "Search", bounds: oldHatSize, windowId: 100)
            let selectedSearch = try uniqueSearchButton(windowId: 99, from: [search, otherWindowSearch])
            var ambiguousSearchRejected = false
            do { _ = try uniqueSearchButton(windowId: 99, from: [search, search]) }
            catch { ambiguousSearchRejected = true }
            guard selectedSearch == search, ambiguousSearchRejected else {
                throw ToolError(message: "pure exact-window Search reference test failed")
            }
            return ["ok": true, "mode": "no-window-self-test", "uiTouched": false, "visibleSecurityAgentRejected": true, "visibleSystemSettingsRejected": true, "ambiguousWindowMappingRejected": true, "exactMainWindowSelectionVerified": true, "liveSearchSizingVerified": true, "ambiguousSearchReferenceRejected": true]
        case "activate":
            guard values.count == 2, let pid = pid_t(values[0]) else { throw ToolError(message: "activate requires PID EXECUTABLE_PATH") }
            let executablePath = standardized(values[1])
            try requireTarget(pid: pid, executablePath: executablePath)
            try requireSafeFrontmost()
            guard let app = NSRunningApplication(processIdentifier: pid) else { throw ToolError(message: "exact target PID is no longer running") }
            guard app.activate(options: [.activateAllWindows]) else {
                throw ToolError(message: "macOS refused activation of exact target PID")
            }
            let deadline = Date().addingTimeInterval(8)
            while Date() < deadline {
                if NSWorkspace.shared.frontmostApplication?.processIdentifier == pid {
                    try requireSafeFrontmost()
                    return ["ok": true, "pid": Int(pid), "frontmostPid": Int(pid)]
                }
                Thread.sleep(forTimeInterval: 0.1)
            }
            throw ToolError(message: "exact target PID did not become frontmost within 8 seconds")
        case "inspect":
            guard values.count == 3, let pid = pid_t(values[0]) else { throw ToolError(message: "inspect requires PID EXECUTABLE_PATH LABELS_JSON") }
            try requireTarget(pid: pid, executablePath: standardized(values[1]))
            try requireSafeFrontmost()
            try requireAXTrust()
            let labels = try decodeLabels(values[2])
            let result = try inspect(pid: pid, labels: labels)
            try requireSafeFrontmost()
            return ["ok": true, "pid": Int(pid), "windows": result.windows.map(\.json), "toggles": result.toggles.map(\.json), "searchButtons": result.searchButtons.map(\.json)]
        case "press":
            guard values.count == 5, let pid = pid_t(values[0]), let expectedWindowId = Int(values[4]), expectedWindowId > 0 else {
                throw ToolError(message: "press requires PID EXECUTABLE_PATH LABELS_JSON KIND CG_WINDOW_ID")
            }
            let path = standardized(values[1])
            let kind = values[3]
            guard kind == "open" || kind == "exit" else { throw ToolError(message: "press KIND must be open or exit") }
            try requireTarget(pid: pid, executablePath: path)
            try requireFrontmost(pid: pid)
            try requireSafeFrontmost()
            try requireAXTrust()
            let labels = try decodeLabels(values[2])
            let first = try inspect(pid: pid, labels: labels)
            guard selectedMainWindowId(first.windows.map(WindowIdentity.init)) == expectedWindowId else {
                throw ToolError(message: "selected main CG window \(expectedWindowId) no longer resolves uniquely; no AXPress was performed")
            }
            guard let firstToggle = try uniqueToggle(kind: kind, windowId: expectedWindowId, from: first.toggles) else {
                throw ToolError(message: "no exact \(kind) AXCheckBox exists in selected CG window \(expectedWindowId); no press was performed")
            }
            let firstSearchButton = try validatePressable(firstToggle, expectedKind: kind, searchButtons: first.searchButtons)

            // Re-read both the primary-window identity and toggle before AXPress. A process-wide label match
            // is insufficient: only the exact main CGWindowID chosen by the TS coordinator may be targeted.
            let current = try inspect(pid: pid, labels: labels)
            guard selectedMainWindowId(current.windows.map(WindowIdentity.init)) == expectedWindowId else {
                throw ToolError(message: "selected main CG window identity changed before AXPress; no action was performed")
            }
            guard let toggle = try uniqueToggle(kind: kind, windowId: expectedWindowId, from: current.toggles) else {
                throw ToolError(message: "selected main window no longer contains one exact \(kind) AXCheckBox; no action was performed")
            }
            let currentSearchButton = try validatePressable(toggle, expectedKind: kind, searchButtons: current.searchButtons)
            guard toggle.label == firstToggle.label, toggle.bounds == firstToggle.bounds,
                  currentSearchButton == firstSearchButton else {
                throw ToolError(message: "selected main-window toggle identity changed before AXPress; no action was performed")
            }
            try requireFrontmost(pid: pid)
            try requireSafeFrontmost()
            let axError = AXUIElementPerformAction(toggle.element, kAXPressAction as CFString)
            guard axError == .success else { throw ToolError(message: "AXPress failed with AXError \(axError.rawValue)") }
            return ["ok": true, "pid": Int(pid), "performed": true, "toggle": toggle.json]
        case "close-window":
            guard values.count == 3, let pid = pid_t(values[0]), let windowId = Int(values[2]) else { throw ToolError(message: "close-window requires PID EXECUTABLE_PATH CG_WINDOW_ID") }
            try requireTarget(pid: pid, executablePath: standardized(values[1]))
            try requireFrontmost(pid: pid)
            try requireSafeFrontmost()
            try requireAXTrust()
            let initialWindows = try readWindows(pid: pid)
            guard selectedMainWindowId(initialWindows.map(WindowIdentity.init)) == windowId else {
                throw ToolError(message: "CG window \(windowId) is not the uniquely selected main window; nothing was closed")
            }
            let windows = try readWindows(pid: pid)
            guard selectedMainWindowId(windows.map(WindowIdentity.init)) == windowId else {
                throw ToolError(message: "selected main-window identity changed before native close; nothing was closed")
            }
            let matches = windows.filter { $0.windowId == windowId }
            guard matches.count == 1, let window = matches.first else { throw ToolError(message: "CG window \(windowId) did not resolve to one exact AXWindow; nothing was closed") }
            let rawCloseButton = try? copyAttribute(window.element, kAXCloseButtonAttribute as CFString)
            guard let rawCloseButton, CFGetTypeID(rawCloseButton as CFTypeRef) == AXUIElementGetTypeID() else {
                throw ToolError(message: "exact AXWindow does not expose its native AXCloseButton; no close was performed")
            }
            let closeButton = rawCloseButton as! AXUIElement
            guard (try? copyAttribute(closeButton, kAXRoleAttribute as CFString) as? String) == (kAXButtonRole as String),
                  bool(try? copyAttribute(closeButton, kAXEnabledAttribute as CFString)) == true,
                  actionNames(closeButton).contains(kAXPressAction as String) else {
                throw ToolError(message: "exact AXWindow native close button is not an enabled AXButton with AXPress; no close was performed")
            }
            try requireFrontmost(pid: pid)
            try requireSafeFrontmost()
            let axError = AXUIElementPerformAction(closeButton, kAXPressAction as CFString)
            guard axError == .success else { throw ToolError(message: "native AXCloseButton press failed with AXError \(axError.rawValue)") }
            return ["ok": true, "pid": Int(pid), "windowId": window.windowId, "performed": true]
        default:
            throw ToolError(message: "unsupported helper command: \(command)")
        }
    }

    private static func matchingApplications(executablePath: String) -> [NSRunningApplication] {
        NSWorkspace.shared.runningApplications.filter { app in
            app.bundleIdentifier == bundleIdentifier &&
                app.executableURL.map { standardized($0.path) == executablePath } == true &&
                !app.isTerminated
        }
    }

    private static func requireTarget(pid: pid_t, executablePath: String) throws {
        let matches = matchingApplications(executablePath: executablePath)
        guard matches.contains(where: { $0.processIdentifier == pid }) else {
            throw ToolError(message: "PID \(pid) is not a running \(bundleIdentifier) process at the exact requested executable path")
        }
    }

    private static func requireFrontmost(pid: pid_t) throws {
        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == pid else {
            throw ToolError(message: "exact target PID \(pid) is not frontmost; no AX action was performed")
        }
    }

    private static func requireSafeFrontmost() throws {
        let dialogs = visibleSystemDialogs()
        let app = NSWorkspace.shared.frontmostApplication
        let failure = safeModalBlocker(
            securityAgentVisible: dialogs.securityAgent,
            settingsVisible: dialogs.systemSettings,
            securityAgentFrontmost: isSecurityAgent(bundleIdentifier: app?.bundleIdentifier, name: app?.localizedName),
            settingsFrontmost: isSystemSettings(bundleIdentifier: app?.bundleIdentifier, name: app?.localizedName)
        )
        guard failure == nil else {
            throw ToolError(message: failure!)
        }
    }

    private static func safeModalBlocker(securityAgentVisible: Bool, settingsVisible: Bool, securityAgentFrontmost: Bool, settingsFrontmost: Bool) -> String? {
        if securityAgentVisible { return "a SecurityAgent window is visible; no activation or AX action was performed" }
        if settingsVisible { return "a System Settings window is visible; no activation or AX action was performed" }
        if securityAgentFrontmost { return "SecurityAgent is frontmost; no activation or AX action was performed" }
        if settingsFrontmost { return "System Settings is frontmost; no activation or AX action was performed" }
        return nil
    }

    private static func selectedMainWindowId(_ windows: [WindowIdentity]) -> Int? {
        let candidates = windows.filter { $0.layer == 0 && $0.alpha > 0 && !$0.isMinimized }
        let main = candidates.filter(\.isMain)
        let selected = main.count == 1 ? main : (candidates.count == 1 ? candidates : [])
        guard selected.count == 1 else { return nil }
        return selected[0].windowId
    }

    private static func exactCGWindowMappingIsUnambiguous(_ count: Int) -> Bool { count == 1 }

    private static func isBlocked(bundleIdentifier: String?, name: String?) -> Bool {
        isSecurityAgent(bundleIdentifier: bundleIdentifier, name: name) || isSystemSettings(bundleIdentifier: bundleIdentifier, name: name)
    }

    private static func isSecurityAgent(bundleIdentifier: String?, name: String?) -> Bool {
        bundleIdentifier == "com.apple.SecurityAgent" || name == "SecurityAgent"
    }

    private static func isSystemSettings(bundleIdentifier: String?, name: String?) -> Bool {
        ["com.apple.systempreferences", "com.apple.SystemPreferences"].contains(bundleIdentifier ?? "") ||
            ["System Settings", "System Preferences"].contains(name ?? "")
    }

    private static func visibleSystemDialogs() -> (securityAgent: Bool, systemSettings: Bool) {
        let apps = NSWorkspace.shared.runningApplications
        let securityPids = Set(apps.filter { isSecurityAgent(bundleIdentifier: $0.bundleIdentifier, name: $0.localizedName) }.map { Int($0.processIdentifier) })
        let settingsPids = Set(apps.filter { isSystemSettings(bundleIdentifier: $0.bundleIdentifier, name: $0.localizedName) }.map { Int($0.processIdentifier) })
        guard let raw = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else {
            return (!securityPids.isEmpty, !settingsPids.isEmpty)
        }
        var securityVisible = false
        var settingsVisible = false
        for window in raw {
            guard let pid = (window[kCGWindowOwnerPID as String] as? NSNumber)?.intValue else { continue }
            if securityPids.contains(pid) { securityVisible = true }
            if settingsPids.contains(pid) { settingsVisible = true }
        }
        return (securityVisible, settingsVisible)
    }

    private static func requireAXTrust() throws {
        guard AXIsProcessTrusted() else {
            throw ToolError(message: "Accessibility is not already authorized for this helper context; no prompt was requested and no AX action was performed")
        }
    }

    private static func decodeLabels(_ json: String) throws -> [String: Set<String>] {
        guard let data = json.data(using: .utf8),
              let object = try JSONSerialization.jsonObject(with: data) as? [String: [String]] else {
            throw ToolError(message: "LABELS_JSON must be an object of string arrays")
        }
        let open = Set(object["open"] ?? [])
        let exit = Set(object["exit"] ?? [])
        let search = Set(object["search"] ?? [])
        guard !open.isEmpty, !exit.isEmpty, !search.isEmpty else { throw ToolError(message: "LABELS_JSON must contain non-empty open, exit, and search arrays") }
        return ["open": open, "exit": exit, "search": search]
    }

    private static func inspect(pid: pid_t, labels: [String: Set<String>]) throws -> (windows: [WindowRecord], toggles: [ToggleRecord], searchButtons: [SearchButtonRecord]) {
        let windows = try readWindows(pid: pid)
        var toggles: [ToggleRecord] = []
        var searchButtons: [SearchButtonRecord] = []
        var visited = 0
        for window in windows {
            collectToggles(in: window.element, window: window, labels: labels, depth: 0, visited: &visited, into: &toggles, searchButtons: &searchButtons)
        }
        return (windows, toggles, searchButtons)
    }

    private static func readWindows(pid: pid_t) throws -> [WindowRecord] {
        let appElement = AXUIElementCreateApplication(pid)
        guard let rawWindows = try copyAttribute(appElement, kAXWindowsAttribute as CFString) as? [AXUIElement] else {
            throw ToolError(message: "target application did not expose an AXWindows array")
        }
        let cgWindows = cgWindowRecords(pid: pid)
        var records: [WindowRecord] = []
        for window in rawWindows {
            guard let position = point(try? copyAttribute(window, kAXPositionAttribute as CFString)),
                  let size = size(try? copyAttribute(window, kAXSizeAttribute as CFString)) else { continue }
            let bounds = Bounds(CGRect(origin: position, size: size))
            let candidates = cgWindows.filter { $0.bounds.distance(to: bounds) <= 8 }
            guard exactCGWindowMappingIsUnambiguous(candidates.count), let candidate = candidates.first else {
                throw ToolError(message: "AXWindow geometry mapped to \(candidates.count) CG windows; refusing ambiguous or absent window identity")
            }
            let actions = actionNames(window)
            records.append(WindowRecord(
                element: window,
                windowId: candidate.windowId,
                bounds: bounds,
                layer: candidate.layer,
                alpha: candidate.alpha,
                zIndex: candidate.zIndex,
                isMain: bool(try? copyAttribute(window, kAXMainAttribute as CFString)) ?? false,
                isMinimized: bool(try? copyAttribute(window, kAXMinimizedAttribute as CFString)) ?? false,
                actions: actions
            ))
        }
        let ids = records.map(\.windowId)
        guard Set(ids).count == ids.count else { throw ToolError(message: "multiple AXWindows mapped to the same CG window ID; refusing ambiguous identity") }
        return records
    }

    private static func cgWindowRecords(pid: pid_t) -> [(windowId: Int, bounds: Bounds, layer: Int, alpha: Double, zIndex: Int)] {
        guard let raw = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] else { return [] }
        var records: [(windowId: Int, bounds: Bounds, layer: Int, alpha: Double, zIndex: Int)] = []
        for (index, item) in raw.enumerated() {
            guard (item[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == pid,
                  let windowId = (item[kCGWindowNumber as String] as? NSNumber)?.intValue,
                  let rect = (item[kCGWindowBounds as String] as? NSDictionary).flatMap(rect(from:)) else { continue }
            records.append((windowId, Bounds(rect), (item[kCGWindowLayer as String] as? NSNumber)?.intValue ?? -1, (item[kCGWindowAlpha as String] as? NSNumber)?.doubleValue ?? 0, index))
        }
        return records
    }

    private static func collectToggles(
        in element: AXUIElement,
        window: WindowRecord,
        labels: [String: Set<String>],
        depth: Int,
        visited: inout Int,
        into toggles: inout [ToggleRecord],
        searchButtons: inout [SearchButtonRecord]
    ) {
        guard depth <= maximumAXDepth, visited < maximumAXNodes else { return }
        visited += 1
        if (try? copyAttribute(element, kAXRoleAttribute as CFString) as? String) == (kAXCheckBoxRole as String),
           let match = matchedLabel(in: element, labels: labels),
           let bounds = elementBounds(element) {
            let rawValue = try? copyAttribute(element, kAXValueAttribute as CFString)
            let safeValue: Any
            if let rawValue = rawValue as? NSNumber { safeValue = rawValue.boolValue }
            else { safeValue = NSNull() }
            toggles.append(ToggleRecord(
                element: element,
                role: kAXCheckBoxRole as String,
                label: match.label,
                kind: match.kind,
                enabled: bool(try? copyAttribute(element, kAXEnabledAttribute as CFString)) ?? false,
                value: safeValue,
                bounds: bounds,
                windowId: window.windowId,
                windowBounds: window.bounds,
                actions: actionNames(element)
            ))
        }
        if (try? copyAttribute(element, kAXRoleAttribute as CFString) as? String) == (kAXButtonRole as String),
           let label = matchedSearchLabel(in: element, labels: labels),
           let bounds = elementBounds(element) {
            searchButtons.append(SearchButtonRecord(role: kAXButtonRole as String, label: label, bounds: bounds, windowId: window.windowId))
        }
        guard let children = try? copyAttribute(element, kAXChildrenAttribute as CFString) as? [AXUIElement] else { return }
        for child in children {
            collectToggles(in: child, window: window, labels: labels, depth: depth + 1, visited: &visited, into: &toggles, searchButtons: &searchButtons)
            if visited >= maximumAXNodes { return }
        }
    }

    private static func matchedLabel(in element: AXUIElement, labels: [String: Set<String>]) -> (kind: String, label: String)? {
        let candidates = [kAXTitleAttribute, kAXDescriptionAttribute, kAXHelpAttribute].compactMap { attribute in
            try? copyAttribute(element, attribute as CFString) as? String
        }
        for kind in ["open", "exit"] {
            guard let expected = labels[kind] else { continue }
            if let value = candidates.first(where: { expected.contains($0) }) { return (kind, value) }
        }
        return nil
    }

    private static func matchedSearchLabel(in element: AXUIElement, labels: [String: Set<String>]) -> String? {
        guard let expected = labels["search"] else { return nil }
        let candidates = [kAXTitleAttribute, kAXDescriptionAttribute, kAXHelpAttribute].compactMap { attribute in
            try? copyAttribute(element, attribute as CFString) as? String
        }
        return candidates.first(where: { expected.contains($0.trimmingCharacters(in: .whitespacesAndNewlines)) })
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
    }

    private static func uniqueToggle(kind: String, windowId: Int, from toggles: [ToggleRecord]) throws -> ToggleRecord? {
        let matches = toggles.filter { $0.kind == kind && $0.windowId == windowId }
        guard matches.count == 1 else { throw ToolError(message: "expected one \(kind) AXCheckBox in CG window \(windowId); found \(matches.count)") }
        return matches.first
    }

    private static func uniqueSearchButton(windowId: Int, from searchButtons: [SearchButtonRecord]) throws -> SearchButtonRecord {
        let matches = searchButtons.filter { $0.windowId == windowId && $0.role == (kAXButtonRole as String) }
        guard matches.count == 1, let button = matches.first else {
            throw ToolError(message: "expected one official Search AXButton in CG window \(windowId); found \(matches.count)")
        }
        return button
    }

    private static func sizesMatchLiveSearch(_ toggleBounds: Bounds, _ searchBounds: Bounds) -> Bool {
        let dimensions = [toggleBounds.width, toggleBounds.height, searchBounds.width, searchBounds.height]
        guard dimensions.allSatisfy({ $0.isFinite && $0 > 0 }) else { return false }
        return abs(toggleBounds.width - searchBounds.width) <= 0.25
            && abs(toggleBounds.height - searchBounds.height) <= 0.25
    }

    @discardableResult
    private static func validatePressable(_ toggle: ToggleRecord, expectedKind: String, searchButtons: [SearchButtonRecord]) throws -> SearchButtonRecord {
        let searchButton = try uniqueSearchButton(windowId: toggle.windowId, from: searchButtons)
        guard toggle.kind == expectedKind,
              toggle.role == (kAXCheckBoxRole as String),
              sizesMatchLiveSearch(toggle.bounds, searchButton.bounds),
              toggle.enabled,
              toggle.actions.contains(kAXPressAction as String) else {
            throw ToolError(message: "exact \(expectedKind) toggle failed role/enabled/live-Search-size/AXPress checks; no action was performed")
        }
        return searchButton
    }

    private static func elementBounds(_ element: AXUIElement) -> Bounds? {
        guard let origin = point(try? copyAttribute(element, kAXPositionAttribute as CFString)),
              let dimensions = size(try? copyAttribute(element, kAXSizeAttribute as CFString)) else { return nil }
        return Bounds(CGRect(origin: origin, size: dimensions))
    }

    private static func actionNames(_ element: AXUIElement) -> [String] {
        var raw: CFArray?
        guard AXUIElementCopyActionNames(element, &raw) == .success else { return [] }
        return raw as? [String] ?? []
    }

    private static func copyAttribute(_ element: AXUIElement, _ attribute: CFString) throws -> Any {
        var value: CFTypeRef?
        let error = AXUIElementCopyAttributeValue(element, attribute, &value)
        guard error == .success, let value else { throw ToolError(message: "AX attribute \(attribute) unavailable (AXError \(error.rawValue))") }
        return value
    }

    private static func point(_ raw: Any?) -> CGPoint? {
        guard let value = raw, CFGetTypeID(value as CFTypeRef) == AXValueGetTypeID() else { return nil }
        let axValue = value as! AXValue
        guard AXValueGetType(axValue) == .cgPoint else { return nil }
        var result = CGPoint.zero
        return AXValueGetValue(axValue, .cgPoint, &result) ? result : nil
    }

    private static func size(_ raw: Any?) -> CGSize? {
        guard let value = raw, CFGetTypeID(value as CFTypeRef) == AXValueGetTypeID() else { return nil }
        let axValue = value as! AXValue
        guard AXValueGetType(axValue) == .cgSize else { return nil }
        var result = CGSize.zero
        return AXValueGetValue(axValue, .cgSize, &result) ? result : nil
    }

    private static func bool(_ raw: Any?) -> Bool? {
        guard let number = raw as? NSNumber else { return nil }
        return number.boolValue
    }

    private static func rect(from dictionary: NSDictionary) -> CGRect? {
        guard let x = (dictionary["X"] as? NSNumber)?.doubleValue,
              let y = (dictionary["Y"] as? NSNumber)?.doubleValue,
              let width = (dictionary["Width"] as? NSNumber)?.doubleValue,
              let height = (dictionary["Height"] as? NSNumber)?.doubleValue else { return nil }
        return CGRect(x: x, y: y, width: width, height: height)
    }

    private static func standardized(_ path: String) -> String {
        URL(fileURLWithPath: path).standardizedFileURL.path
    }

    private static func writeJSON(_ object: [String: Any]) throws {
        let data = try JSONSerialization.data(withJSONObject: object, options: [.fragmentsAllowed, .sortedKeys])
        guard let line = String(data: data, encoding: .utf8) else { throw ToolError(message: "could not encode output as UTF-8") }
        print(line)
    }
}

Helper.main()
