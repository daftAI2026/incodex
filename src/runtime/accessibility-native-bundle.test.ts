import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { runInNewContext } from "node:vm";
import { createHash } from "node:crypto";
import { expect, test } from "bun:test";
import { RUNTIME_EXTERNAL_ARTIFACT_NAMES } from "../runtime-manifest.ts";

const SHARED_PERMISSION_UI = "incodex-permission-ui.cjs";

test("the published main loads the native guide and its cancellable handoff", async () => {
  const filename = new URL("../../dist/incodex-main.cjs", import.meta.url);
  const source = readFileSync(filename, "utf8");
  const uiSource = readFileSync(new URL("../../dist/incodex-permission-ui.cjs", import.meta.url), "utf8");
  expect(uiSource).toContain("Native permission dylib hash mismatch");
  expect(uiSource).toContain("updateProgress$cornerRadius$reduceTransparency$");
  expect(source).not.toMatch(/require\(["']\.\/incodex-permission-[^"']+\.cts["']\)/);
  const require = createRequire(filename);
  const api = runInNewContext(`${readFileSync(filename, "utf8")}\n;accessibilityWindow`, {
    require(name: string) {
      if (name === "electron") throw new Error("Do not attach the Runtime in a build test");
      return require(name);
    },
    exports: {}, module: { exports: {} }, process, Buffer, console,
    __dirname: new URL("../../dist/", import.meta.url).pathname,
    setTimeout, clearTimeout, setInterval, clearInterval, performance,
  });
  expect(typeof api.createNativeAccessibilitySetupWindow).toBe("function");
  const flight = api.runNativePermissionHandoff({ reducedMotion: true });
  expect(typeof flight.dispose).toBe("function");
  await flight.finished;
});

test("the compatibility main loads one verified sibling permission UI asset", async () => {
  const mainFilename = new URL("../../dist/incodex-main.cjs", import.meta.url);
  const uiFilename = new URL(`../../dist/${SHARED_PERMISSION_UI}`, import.meta.url);
  const manifest = JSON.parse(readFileSync(new URL("../../dist/runtime-manifest.json", import.meta.url), "utf8"));
  const mainSource = readFileSync(mainFilename, "utf8");
  const uiBytes = readFileSync(uiFilename);
  const uiSource = uiBytes.toString();
  const presenterMarker = "Native permission SwiftUI guide classes are unavailable";
  const flightMarker = "Native permission SwiftUI flight class is unavailable";

  expect(RUNTIME_EXTERNAL_ARTIFACT_NAMES).toContain(SHARED_PERMISSION_UI);
  expect(manifest.files[SHARED_PERMISSION_UI]).toBe(createHash("sha256").update(uiBytes).digest("hex"));
  expect(mainSource).toContain(`require("./${SHARED_PERMISSION_UI}")`);
  expect(mainSource).not.toContain(presenterMarker);
  expect(mainSource).not.toContain(flightMarker);
  expect(uiSource).toContain(presenterMarker);
  expect(uiSource).toContain(flightMarker);

  const uiRequire = createRequire(uiFilename);
  const shared = uiRequire(uiFilename.pathname);
  expect(typeof shared.createNativeAccessibilitySetupWindow).toBe("function");
  expect(typeof shared.runNativePermissionHandoff).toBe("function");
  const reducedMotionFlight = shared.runNativePermissionHandoff({ reducedMotion: true });
  expect(typeof reducedMotionFlight.dispose).toBe("function");
  await reducedMotionFlight.finished;

  const mainRequire = createRequire(mainFilename);
  const mainApi = runInNewContext(`${mainSource}\n;accessibilityWindow`, {
    require(name: string) {
      if (name === "electron") throw new Error("Do not attach the Runtime in a build test");
      return mainRequire(name);
    },
    exports: {}, module: { exports: {} }, process, Buffer, console,
    __dirname: new URL("../../dist/", import.meta.url).pathname,
    setTimeout, clearTimeout, setInterval, clearInterval, performance,
  });
  expect(mainApi.createNativeAccessibilitySetupWindow).toBe(shared.createNativeAccessibilitySetupWindow);

});
