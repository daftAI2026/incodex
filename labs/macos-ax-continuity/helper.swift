/**
 * [INPUT]: 依赖 AppKit/ApplicationServices 的当前进程验权与 Finder 公共菜单读取，候选身份来自包内资源
 * [OUTPUT]: 提供一次性原生 AX 实验宿主与脱敏 JSON 后置条件，不返回窗口标题或用户内容
 * [POS]: macos-ax-continuity 的真实权限探针；v1/v2 内容不同但签名身份相同，不能替代 Codex 验权
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import AppKit
import ApplicationServices
import Foundation

#if GENERATION_TWO
let generation = "v2"
#else
let generation = "v1"
#endif
let candidateID = "INCODEX-V1.1-MAC-AX-RC1"

func collectAXObservation() -> [String: Any] {
    let trusted = AXIsProcessTrustedWithOptions(
        [kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String: false] as CFDictionary
    )
    var result: [String: Any] = [
        "schemaVersion": 1, "candidateId": candidateID, "generation": generation,
        "pid": ProcessInfo.processInfo.processIdentifier,
        "bundleId": Bundle.main.bundleIdentifier ?? "unbundled",
        "selfTrusted": trusted, "targetBundleId": "com.apple.finder",
        "actualAXRead": "NOT_RUN", "observedAt": ISO8601DateFormatter().string(from: Date()),
        "codexPermissionClaim": false,
    ]
    // 仅访问 Finder 的公共菜单节点；不枚举窗口、文件、聊天或登录信息。
    let targets = NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.finder")
    if targets.count == 1 {
        let target = targets[0]
        let element = AXUIElementCreateApplication(target.processIdentifier)
        var menu: CFTypeRef?
        let status = AXUIElementCopyAttributeValue(element, kAXMenuBarAttribute as CFString, &menu)
        result["targetPID"] = target.processIdentifier
        result["axError"] = status.rawValue
        result["actualAXRead"] = status == .success && menu != nil ? "PASS" : "FAIL"
    } else {
        result["actualAXRead"] = "TARGET_NOT_UNIQUE"
    }
    return result
}

func serialize(_ value: [String: Any]) -> Data {
    (try? JSONSerialization.data(withJSONObject: value, options: [.prettyPrinted, .sortedKeys])) ?? Data()
}

func reportURL() -> URL? {
    let arguments = CommandLine.arguments
    guard let index = arguments.firstIndex(of: "--report"), index + 1 < arguments.count else { return nil }
    let url = URL(fileURLWithPath: arguments[index + 1])
    let parent = url.deletingLastPathComponent()
    guard let attributes = try? FileManager.default.attributesOfItem(atPath: parent.path),
          attributes[.type] as? FileAttributeType == .typeDirectory,
          (attributes[.ownerAccountID] as? NSNumber)?.uint32Value == getuid(),
          (attributes[.posixPermissions] as? NSNumber)?.intValue == 0o700,
          parent.resolvingSymlinksInPath().path == parent.standardizedFileURL.path else { return nil }
    return url
}

final class LabDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    var window: NSWindow?
    var statusLabel: NSTextField?
    var timer: Timer?
    let output = reportURL()

    func applicationDidFinishLaunching(_ notification: Notification) {
        let panel = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 600, height: 290),
                             styleMask: [.titled, .closable], backing: .buffered, defer: false)
        panel.title = "Incodex AX Continuity Lab · \(generation)"
        panel.delegate = self
        panel.isReleasedWhenClosed = false
        let view = NSView(frame: panel.contentView!.bounds)
        let title = NSTextField(labelWithString: "\(candidateID) · \(generation)")
        title.font = .boldSystemFont(ofSize: 17)
        title.frame = NSRect(x: 24, y: 225, width: 550, height: 28)
        view.addSubview(title)
        let body = NSTextField(wrappingLabelWithString:
            "这是独立实验，不改变 Codex。仅对本实验应用批准一次辅助功能权限；随后换成同一签名身份的 v2，验证无需再次批准。系统认证由你完成。探针只读取 Finder 公共菜单节点。")
        body.frame = NSRect(x: 24, y: 124, width: 550, height: 86)
        view.addSubview(body)
        let label = NSTextField(wrappingLabelWithString: "正在只读验权……")
        label.frame = NSRect(x: 24, y: 67, width: 550, height: 45)
        view.addSubview(label)
        statusLabel = label
        let settings = NSButton(title: "打开辅助功能设置", target: self, action: #selector(openSettings))
        settings.frame = NSRect(x: 24, y: 21, width: 180, height: 32)
        view.addSubview(settings)
        let close = NSButton(title: "结束本轮", target: self, action: #selector(finish))
        close.frame = NSRect(x: 443, y: 21, width: 125, height: 32)
        view.addSubview(close)
        panel.contentView = view
        panel.center()
        panel.makeKeyAndOrderFront(nil)
        NSApplication.shared.activate(ignoringOtherApps: true)
        window = panel
        refresh()
        timer = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in self?.refresh() }
    }

    func refresh() {
        let value = collectAXObservation()
        let trusted = value["selfTrusted"] as? Bool == true
        statusLabel?.stringValue = "本进程授权：\(trusted ? "granted" : "denied") · 实际 AX 读取：\(value["actualAXRead"] ?? "unknown")\n不代表 Codex 宿主已获权或跨版本保留已通过。"
        if let output {
            do {
                try serialize(value).write(to: output, options: .atomic)
                try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: output.path)
            } catch {
                statusLabel?.stringValue = "证据写入失败，不能宣布本轮通过。"
            }
        }
    }

    @objc func openSettings() {
        guard let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility") else { return }
        NSWorkspace.shared.open(url) // 仅响应用户点击；不 reset、不代填系统批准。
    }

    @objc func finish() { NSApplication.shared.terminate(nil) }
    func windowWillClose(_ notification: Notification) { timer?.invalidate(); NSApplication.shared.terminate(nil) }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}

if CommandLine.arguments.contains("--probe") {
    FileHandle.standardOutput.write(serialize(collectAXObservation()))
    FileHandle.standardOutput.write(Data([10]))
} else {
    let application = NSApplication.shared
    let delegate = LabDelegate()
    application.setActivationPolicy(.regular)
    application.delegate = delegate
    application.run()
    withExtendedLifetime(delegate) {}
}
