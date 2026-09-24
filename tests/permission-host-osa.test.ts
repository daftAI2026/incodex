import { expect, test } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { tmpdir } from "node:os";

const nativeRoot = join(import.meta.dir, "../native/macos");

test.skipIf(process.platform !== "darwin")(
  "native host preserves the nonce/stdin protocol through its Swift presenter adapter",
  () => {
    const directory = mkdtempSync(join(tmpdir(), "incodex-native-stdio-"));
    try {
      const fixture = join(directory, "presenter.swift");
      const output = join(directory, "host");
      writeFileSync(fixture, `import AppKit
@MainActor
final class PermissionHostPresenter {
    private let onEvent: (String) -> Void
    init(copy: NSDictionary, layoutDirection: String, onEvent: @escaping (String) -> Void, onError: @escaping (String) -> Void) {
        self.onEvent = onEvent
    }
    func present() -> Bool { true }
    func setState(_ state: String, message: String?) {}
    func close() {}
}`);
      const built = spawnSync("xcrun", ["swiftc", "-D", "INCODEX_PERMISSION_HOST_STUB",
        join(nativeRoot, "permission-host.swift"), fixture, "-o", output], { encoding: "utf8" });
      expect(built.status, built.stderr).toBe(0);

      const nonce = "0123456789abcdef0123456789abcdef";
      const input = `${[
        { nonce, type: "configure", copy: {}, layoutDirection: "leftToRight" },
        { nonce, type: "state", state: "granted" },
        { nonce, type: "close" },
      ].map(value => JSON.stringify(value)).join("\n")}\n`;
      const result = spawnSync(output, ["--nonce", nonce], { input, encoding: "utf8", timeout: 10_000 });
      expect(result.status, result.stderr + result.stdout).toBe(0);
      const events = result.stdout.trim().split("\n").map(line => JSON.parse(line));
      expect(events).toEqual([{ nonce, type: "ready" }]);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  },
  120_000,
);
