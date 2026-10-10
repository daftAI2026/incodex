import { expect, test } from "bun:test";
import { installRemoteKeyCompatibility } from "./incodex-remote-key-compat.cts";

test("remote key compatibility never loads native code for private windows or Windows", () => {
  let calls = 0;
  const load = () => { calls++; throw new Error("must not load"); };
  expect(installRemoteKeyCompatibility({ platform: "win32", incognito: false, load })).toBe(false);
  expect(installRemoteKeyCompatibility({ platform: "darwin", incognito: true, load })).toBe(false);
  expect(calls).toBe(0);
});

test("native compatibility failure never prevents official startup", () => {
  expect(installRemoteKeyCompatibility({ platform: "darwin", incognito: false, directory: "/missing" })).toBe(false);
});
