import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const repo = join(import.meta.dir, "..");
const views = join(repo, "native", "macos", "permission-views.swift");
const presenter = join(repo, "native", "macos", "permission-host-presenter.swift");

// This is deliberately a compile-first smoke.  The opt-in execution branch is
// reserved for the owner-controlled native-window run; the default test never
// creates or orders a panel on the user's desktop.
test.skipIf(process.platform !== "darwin")(
  "PermissionHostPresenter compiles against the existing SwiftUI wrapper ABI",
  () => {
    const directory = mkdtempSync(join(tmpdir(), "incodex-permission-host-presenter-"));
    try {
      const executable = join(directory, "presenter-smoke");
      const harness = join(directory, "presenter-smoke.swift");
      writeFileSync(harness, `
import AppKit
import Foundation

@MainActor
@main
enum PermissionHostPresenterSmoke {
    static func main() {
        _ = NSApplication.shared
        let copy: NSDictionary = [
            "title": "Enable ChatGPT scripting",
            "body": "Allow Accessibility access.",
            "permissionTitle": "Accessibility",
            "permissionDescription": "Read and control app interfaces",
            "repair": "Allow",
            "later": "Skip",
            "completeInSettings": "Complete in Settings",
            "back": "Back",
            "dragInstruction": "Drag ChatGPT into the app list above.",
        ]
        let presenter = PermissionHostPresenter(
            copy: copy,
            layoutDirection: "leftToRight",
            onEvent: { _ in },
            onError: { _ in },
        )
        // These calls are typechecked in every run; execution is opt-in so
        // this test remains headless by default.
        if ProcessInfo.processInfo.environment["INCODEX_RUN_PERMISSION_HOST_PRESENTER_SMOKE"] == "1" {
            _ = presenter.present()
            presenter.setState("repairing", message: nil)
            presenter.setState("awaiting-user", message: nil)
            presenter.close()
        }
        print("permission presenter ABI compiled")
    }
}
`);

      const architecture = process.arch === "arm64" ? "arm64" : process.arch === "x64" ? "x86_64" : "";
      expect(architecture).not.toBe("");
      const build = spawnSync("xcrun", [
        "swiftc",
        "-parse-as-library",
        "-target",
        `${architecture}-apple-macos12`,
        "-module-name",
        "IncodexPermissionHostPresenterTest",
        views,
        presenter,
        harness,
        "-o",
        executable,
      ], { cwd: repo, encoding: "utf8", timeout: 90_000 });
      const output = `${build.stdout ?? ""}${build.stderr ?? ""}`;
      expect(build.status, output || String(build.error ?? "Swift presenter compilation failed")).toBe(0);

      if (process.env.INCODEX_RUN_PERMISSION_HOST_PRESENTER_SMOKE === "1") {
        const run = spawnSync(executable, [], { cwd: repo, encoding: "utf8", timeout: 20_000 });
        const runOutput = `${run.stdout ?? ""}${run.stderr ?? ""}`;
        expect(run.status, runOutput || String(run.error ?? "Swift presenter smoke failed")).toBe(0);
        expect(run.stdout).toContain("permission presenter ABI compiled");
      }
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
  120_000,
);
