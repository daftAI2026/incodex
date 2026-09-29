import { afterAll, beforeAll, expect, setDefaultTimeout, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { tmpdir } from "node:os";

const repositoryRoot = join(import.meta.dir, "..");
const nativeRoot = join(repositoryRoot, "native", "macos");
const runVisibleSmoke = process.env.INCODEX_RUN_PERMISSION_ACTIVATION_POLICY_AX_SMOKE === "1";

setDefaultTimeout(90_000);

let smoke: { executable: string; directory: string } | undefined;

beforeAll(() => {
  if (process.platform === "darwin") smoke = buildSmoke();
});

afterAll(() => {
  if (smoke) rmSync(smoke.directory, { recursive: true, force: true });
});

test.skipIf(process.platform !== "darwin")(
  "compiles the opt-in activation-policy AX smoke without opening windows",
  () => {
    expect(smoke?.executable).toBeTruthy();
  },
);

test.skipIf(process.platform !== "darwin" || !runVisibleSmoke)(
  "helper Back AXPress works while prohibited and restores accessory policy",
  () => {
    const result = spawnSync(smoke!.executable, [], {
      cwd: repositoryRoot,
      encoding: "utf8",
      timeout: 45_000,
    });
    const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;
    expect(result.status, output || String(result.error ?? "activation-policy AX smoke failed")).toBe(0);
    expect(output).toContain("ACTIVATION_POLICY_AX prohibited=confirmed");
    expect(output).toContain("AXPress=success restored=accessory helperDisposed=yes initialPage=yes");
  },
  60_000,
);

function buildSmoke(): { executable: string; directory: string } {
  const directory = mkdtempSync(join(tmpdir(), "incodex-activation-policy-ax-"));
  const executable = join(directory, "permission-activation-policy-ax-smoke");
  const architecture = process.arch === "arm64" ? "arm64" : "x86_64";
  const result = spawnSync("xcrun", [
    "swiftc",
    "-parse-as-library",
    "-module-name",
    "IncodexPermissionActivationPolicyAXSmoke",
    "-target",
    `${architecture}-apple-macos12.0`,
    join(nativeRoot, "permission-views.swift"),
    join(nativeRoot, "permission-host-settings.swift"),
    join(nativeRoot, "permission-host-flight.swift"),
    join(nativeRoot, "permission-host-presenter.swift"),
    join(import.meta.dir, "native", "permission-activation-policy-ax-smoke.swift"),
    "-framework",
    "ApplicationServices",
    "-framework",
    "CoreGraphics",
    "-o",
    executable,
  ], {
    cwd: repositoryRoot,
    encoding: "utf8",
    timeout: 75_000,
  });
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;
  expect(result.status, output || String(result.error ?? "Swift activation-policy AX smoke failed to compile")).toBe(0);
  return { executable, directory };
}
