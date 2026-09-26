import { expect, test } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { tmpdir } from "node:os";

const root = join(import.meta.dir, "..");
const native = join(root, "native", "macos");
const runVisible = process.env.INCODEX_RUN_PERMISSION_HOST_G12 === "1";

type BuiltSmoke = { executable: string; directory: string };

function buildSmoke(): BuiltSmoke {
  const directory = mkdtempSync(join(tmpdir(), "incodex-permission-host-g12-"));
  const executable = join(directory, "permission-host-g12-smoke");
  const architecture = process.arch === "arm64" ? "arm64" : "x86_64";
  const build = spawnSync("xcrun", [
    "swiftc", "-parse-as-library", "-module-name", "IncodexPermissionHostG12Smoke",
    "-target", `${architecture}-apple-macos12.0`,
    join(native, "permission-views.swift"),
    join(native, "permission-host-settings.swift"),
    join(native, "permission-host-flight.swift"),
    join(native, "permission-host-presenter.swift"),
    join(import.meta.dir, "native", "permission-host-g12-lifecycle-smoke.swift"),
    "-framework", "ApplicationServices", "-framework", "CoreGraphics",
    "-o", executable,
  ], { cwd: root, encoding: "utf8", timeout: 90_000 });
  const output = `${build.stdout ?? ""}${build.stderr ?? ""}`;
  expect(build.status, output || String(build.error ?? "G12 native presenter smoke failed to compile")).toBe(0);
  return { executable, directory };
}

test.skipIf(process.platform !== "darwin")(
  "G12 current Swift presenter lifecycle smoke compiles without opening a window by default",
  () => {
    const built = buildSmoke();
    try {
      expect(built.executable.length).toBeGreaterThan(0);
    } finally {
      rmSync(built.directory, { recursive: true, force: true });
    }
  },
  120_000,
);

test.skipIf(process.platform !== "darwin" || !runVisible)(
  "G12 current Swift presenter clears windows and timers on close, error, and granted",
  () => {
    const built = buildSmoke();
    try {
      const run = spawnSync(built.executable, [], {
        cwd: root,
        encoding: "utf8",
        timeout: 45_000,
      });
      const output = `${run.stdout ?? ""}${run.stderr ?? ""}`;
      expect(run.status, output || String(run.error ?? "G12 visible lifecycle smoke failed")).toBe(0);
      expect(output).toContain("G12_LIFECYCLE close=zero-windows-repeat-safe arrowTimer=nil lateWindow=none");
      expect(output).toContain("error=one-guide-window flightDisposed=yes helperArrow=zero");
      expect(output).toContain("granted=zero-windows flightDisposed=yes timers=nil");
    } finally {
      rmSync(built.directory, { recursive: true, force: true });
    }
  },
  60_000,
);
