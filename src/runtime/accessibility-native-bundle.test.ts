import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { runInNewContext } from "node:vm";
import { expect, test } from "bun:test";

test("the published main embeds the native guide and its cancellable handoff", async () => {
  const filename = new URL("../../dist/incodex-main.cjs", import.meta.url);
  const source = readFileSync(filename, "utf8");
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
