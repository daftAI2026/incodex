import { describe, expect, test } from "bun:test";
import {
  QUIT_APPLESCRIPT,
  STILL_RUNNING_MESSAGE,
  quitOutcome,
  requestAppleQuit,
  waitUntilOfficialGone,
} from "./quit-official";

describe("official quit", () => {
  test("sends an Apple Event quit by bundle id, not kill", () => {
    const cmds: string[][] = [];
    requestAppleQuit(((cmd: string, args: string[]) => {
      cmds.push([cmd, ...args]);
      return { status: 0, stderr: "" };
    }) as typeof import("node:child_process").spawnSync);
    expect(cmds).toEqual([["osascript", "-e", QUIT_APPLESCRIPT]]);
    expect(QUIT_APPLESCRIPT).toContain('application id "com.openai.codex"');
    expect(cmds[0]?.join(" ")).not.toContain("kill");
  });

  test("still-running when the process remains after quit", () => {
    expect(quitOutcome([])).toBe("gone");
    expect(quitOutcome([52038])).toBe("still-running");
  });

  test("userCanceledErr (-128) with a live pid is still-running", () => {
    const ran = requestAppleQuit((() => ({
      status: 1,
      stderr: "execution error: User canceled. (-128)\n",
    })) as unknown as typeof import("node:child_process").spawnSync);
    expect(ran.status).toBe(1);
    expect(ran.stderr).toContain("-128");
    expect(quitOutcome([52038])).toBe("still-running");
    expect(STILL_RUNNING_MESSAGE).toContain("still running");
  });

  test("waitUntilOfficialGone times out while pids remain", () => {
    let now = 0;
    const gone = waitUntilOfficialGone({
      listPids: () => [52038],
      sleepMs: () => {
        now += 200;
      },
      now: () => now,
      timeoutMs: 400,
      intervalMs: 200,
    });
    expect(gone).toBe(false);
  });

  test("waitUntilOfficialGone returns once pids disappear", () => {
    let ticks = 0;
    const gone = waitUntilOfficialGone({
      listPids: () => {
        ticks += 1;
        return ticks < 3 ? [52038] : [];
      },
      sleepMs: () => {},
      now: () => ticks * 200,
      timeoutMs: 5_000,
      intervalMs: 200,
    });
    expect(gone).toBe(true);
  });
});
