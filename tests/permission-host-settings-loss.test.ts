/**
 * [INPUT]: 依赖真实 Swift Presenter、AppKit 及脚本化 Settings locator。
 * [OUTPUT]: 验证双行暂失恢复与 legacy 单行 Later 正控制。
 * [POS]: 原生 Presenter 行为回归；只创建进程内隐藏窗口，不读 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { expect, test } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = join(import.meta.dir, "..");
const native = join(root, "native", "macos");

const harness = `
import AppKit
import CoreGraphics
import Foundation

@MainActor
struct PermissionHostSettingsFrame {
    let frame: NSRect
    let pid: pid_t
    let windowID: CGWindowID
}

@MainActor
final class SettingsLocator {
    static let bundleIdentifier = "com.apple.systempreferences"
    static var sequence: [PermissionHostSettingsFrame?] = []
    static var calls = 0

    static func install(_ sequence: [PermissionHostSettingsFrame?]) {
        self.sequence = sequence
        calls = 0
    }

    func locate() -> PermissionHostSettingsFrame? {
        defer { Self.calls += 1 }
        guard Self.calls < Self.sequence.count else { return nil }
        return Self.sequence[Self.calls]
    }

    func prepareHandoff() {}
}

@MainActor
@main
struct PermissionHostSettingsLossSmoke {
    private static let copy: NSDictionary = [
        "title": "Permission guide",
        "body": "Allow access in System Settings.",
        "permissionTitle": "Accessibility",
        "permissionDescription": "Read and control app interfaces",
        "repair": "Allow",
        "later": "Later",
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
        "screenTitle": "Screen Recording",
        "screenDescription": "Capture the app window",
        "screenAddedTitle": "ChatGPT added",
        "screenAddedBody": "ChatGPT is in the list.",
        "screenDragInstruction": "Drag ChatGPT into the app list above.",
        "screenChecking": "Checking",
    ]

    static func main() {
        let mode = CommandLine.arguments.last ?? "dual"
        precondition(mode == "dual" || mode == "legacy", "expected dual or legacy mode")

        let application = NSApplication.shared
        precondition(application.setActivationPolicy(.accessory))
        application.finishLaunching()
        let visibleBefore = application.windows.filter(\\.isVisible).count
        precondition(visibleBefore == 0, "the isolated smoke started with a visible window")

        let targetA = PermissionHostSettingsFrame(
            frame: NSRect(x: 40, y: 50, width: 900, height: 700), pid: getpid(), windowID: 1,
        )
        let targetB = PermissionHostSettingsFrame(
            frame: NSRect(x: 80, y: 90, width: 900, height: 700), pid: getpid(), windowID: 2,
        )
        var sequence: [PermissionHostSettingsFrame?] = [targetA]
        sequence.append(contentsOf: Array(repeating: nil, count: 10))
        sequence.append(targetB)
        SettingsLocator.install(sequence)

        var events: [String] = []
        let presenter = PermissionHostPresenter(
            copy: copy,
            layoutDirection: "leftToRight",
            onEvent: { events.append($0) },
            onError: { _ in },
        )
        let dual = mode == "dual"
        if dual {
            presenter.configurePermissions(["accessibility": "not-granted", "screen": "not-granted"])
            presenter.activePermission = "screen"
        }
        presenter.state = "awaiting-user"
        presenter.presented = true

        let panel = NSPanel(
            contentRect: NSRect(x: 40, y: 40, width: 531, height: 110),
            styleMask: [], backing: .buffered, defer: false,
        )
        let helper = IncodexPermissionHelperView(frame: NSRect(x: 0, y: 0, width: 531, height: 110))
        helper.configure(copy: copy, appIcon: NSImage(size: NSSize(width: 32, height: 32)), actionTarget: nil)
        panel.contentView = helper
        presenter.helperPanel = panel
        presenter.helperView = helper

        presenter.startSettingsTracking()
        let deadline = Date().addingTimeInterval(4)
        while Date() < deadline && SettingsLocator.calls < 12 && !presenter.closed {
            RunLoop.current.run(until: Date().addingTimeInterval(0.02))
        }

        let observed = [
            "mode=\\(mode)",
            "calls=\\(SettingsLocator.calls)",
            "later=\\(events.filter { $0 == "later" }.count)",
            "closed=\\(presenter.closed)",
            "helper=\\(presenter.helperPanel != nil)",
            "attempts=\\(presenter.locatingAttempts)",
            "visible=\\(application.windows.filter(\\.isVisible).count)",
        ].joined(separator: " ")
        if !presenter.closed { presenter.close() }
        precondition(application.windows.filter(\\.isVisible).isEmpty, "the isolated smoke left a visible window")
        print("SETTINGS_LOSS \\(observed)")
    }
}
`;

test.skipIf(process.platform !== "darwin")(
  "dual-row Settings loss waits for recovery; legacy single-row keeps its Later behavior",
  () => {
    const directory = mkdtempSync(join(tmpdir(), "incodex-permission-settings-loss-"));
    try {
      const harnessPath = join(directory, "permission-host-settings-loss.swift");
      const executable = join(directory, "permission-host-settings-loss");
      writeFileSync(harnessPath, harness);
      const architecture = process.arch === "arm64" ? "arm64" : process.arch === "x64" ? "x86_64" : "";
      expect(architecture).not.toBe("");
      const build = spawnSync("xcrun", [
        "swiftc", "-parse-as-library", "-module-name", "IncodexPermissionHostSettingsLossSmoke",
        "-target", `${architecture}-apple-macos12.0`,
        join(native, "permission-views.swift"),
        join(native, "permission-initial.swift"),
        join(native, "permission-host-flight.swift"),
        join(native, "permission-host-presenter-geometry.swift"),
        join(native, "permission-host-presenter-views.swift"),
        join(native, "permission-host-presenter.swift"),
        harnessPath,
        "-framework", "ApplicationServices", "-framework", "CoreGraphics",
        "-o", executable,
      ], { cwd: root, encoding: "utf8", timeout: 90_000 });
      const buildOutput = `${build.stdout ?? ""}${build.stderr ?? ""}`;
      expect(build.status, buildOutput || String(build.error ?? "native Settings-loss smoke failed to compile")).toBe(0);
      expect(buildOutput).not.toContain("warning:");
      if (build.status !== 0) return;

      const legacy = spawnSync(executable, ["legacy"], { cwd: root, encoding: "utf8", timeout: 10_000 });
      const legacyOutput = `${legacy.stdout ?? ""}${legacy.stderr ?? ""}`;
      expect(legacy.status, legacyOutput || String(legacy.error ?? "legacy presenter smoke failed")).toBe(0);
      expect(legacy.stdout).toContain("SETTINGS_LOSS mode=legacy calls=11 later=1 closed=true helper=false attempts=0 visible=0");

      const dual = spawnSync(executable, ["dual"], { cwd: root, encoding: "utf8", timeout: 10_000 });
      const dualOutput = `${dual.stdout ?? ""}${dual.stderr ?? ""}`;
      expect(dual.status, dualOutput || String(dual.error ?? "dual-row presenter smoke failed")).toBe(0);
      expect(
        dual.stdout,
        `actual Swift presenter trace after Settings A → nil×10 → B:\n${dualOutput}`,
      ).toContain("SETTINGS_LOSS mode=dual calls=12 later=0 closed=false helper=true attempts=0 visible=0");
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
  120_000,
);
