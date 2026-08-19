import { describe, expect, test } from "bun:test";
import { LSREGISTER, notifyLaunchServices } from "./launch-services";

describe("notifyLaunchServices", () => {
  test("re-registers the app then restarts Dock", () => {
    const calls: Array<{ cmd: string; args: string[] }> = [];
    notifyLaunchServices("/Applications/ChatGPT.app", (cmd, args) => {
      calls.push({ cmd: String(cmd), args: args as string[] });
      return { status: 0 } as never;
    });
    expect(calls).toEqual([
      { cmd: LSREGISTER, args: ["-f", "/Applications/ChatGPT.app"] },
      { cmd: "/usr/bin/killall", args: ["Dock"] },
    ]);
  });
});

