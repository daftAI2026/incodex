import { describe, expect, test } from "bun:test";
import { startSpinner } from "./spinner";

describe("spinner", () => {
  test("tty spinner writes a frame and clears the line on stop", () => {
    const writes: string[] = [];
    const ticks: Array<() => void> = [];
    const spinner = startSpinner("Waiting for the window to close", {
      tty: true,
      write: (text) => writes.push(text),
      setInterval: (fn: () => void) => {
        ticks.push(fn);
        return 1 as unknown as ReturnType<typeof setInterval>;
      },
      clearInterval: () => {},
    });
    expect(writes[0]).toContain("Waiting for the window to close");
    expect(writes[0]).toContain("\r");
    ticks[0]?.();
    expect(writes.length).toBeGreaterThan(1);
    spinner.stop();
    expect(writes.at(-1)).toContain("\u001b[2K");
  });

  test("non-tty spinner does not write", () => {
    const writes: string[] = [];
    const spinner = startSpinner("Installing", {
      tty: false,
      write: (text) => writes.push(text),
    });
    spinner.stop();
    expect(writes).toEqual([]);
  });
});
