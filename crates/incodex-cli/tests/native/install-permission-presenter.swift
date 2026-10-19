/**
 * [INPUT]: 接收真实 native host 入口的权限状态与 Presenter 回调。
 * [OUTPUT]: 提供无窗 Presenter，并经入口回传 Skip 与窗口关闭事件。
 * [POS]: 安装权限整链测试夹具；只替换 UI，不探测 TCC 或系统设置。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import AppKit
import Foundation

@MainActor
final class PermissionHostPresenter {
    private let onEvent: (String) -> Void
    private let onError: (String) -> Void
    private let controlFile: URL
    private var controlTimer: DispatchSourceTimer?
    private var controlConsumed = false
    private var presented = false
    private var closed = false

    init(
        copy: NSDictionary,
        layoutDirection: String,
        onEvent: @escaping (String) -> Void,
        onError: @escaping (String) -> Void,
    ) {
        self.onEvent = onEvent
        self.onError = onError
        let executable = URL(fileURLWithPath: CommandLine.arguments[0]).standardizedFileURL
        self.controlFile = URL(fileURLWithPath: executable.path + ".control")
    }

    func configurePermissions(_ statuses: [String: String]) {}

    func updatePermissions(_ statuses: [String: String]) {}

    func setPermissionState(_ state: String, permission: String, message: String?) {}

    func setState(_ state: String, message: String?) {}

    @discardableResult
    func present() -> Bool {
        guard !closed else { return false }
        guard !presented else { return true }
        presented = true

        let timer = DispatchSource.makeTimerSource(queue: .main)
        timer.schedule(deadline: .now() + .milliseconds(20), repeating: .milliseconds(20))
        timer.setEventHandler { [weak self] in self?.readControlFile() }
        controlTimer = timer
        timer.resume()
        return true
    }

    func close() {
        guard !closed else { return }
        closed = true
        controlTimer?.setEventHandler {}
        controlTimer?.cancel()
        controlTimer = nil
    }

    private func readControlFile() {
        guard !closed, !controlConsumed,
              let data = try? Data(contentsOf: controlFile), data.count <= 64,
              let raw = String(data: data, encoding: .utf8) else { return }

        let action = raw.trimmingCharacters(in: .newlines)
        guard raw == action || raw == action + "\n" || raw == action + "\r\n" else {
            rejectControlFile()
            return
        }

        switch action {
        case "skip", "window-close":
            controlConsumed = true
            // Skip 与窗口 X 在真实 Presenter 中都向入口报告 Later。
            onEvent("later")
            close()
        default:
            rejectControlFile()
        }
    }

    private func rejectControlFile() {
        guard !controlConsumed else { return }
        controlConsumed = true
        onError("unsupported permission presenter test action")
        close()
    }
}
