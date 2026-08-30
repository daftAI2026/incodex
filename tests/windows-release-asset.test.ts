import { describe, expect, test } from "bun:test";

import {
  WINDOWS_X64_MACHINE,
  peMachine,
} from "../scripts/build-windows-release";

function peFixture(machine: number): Uint8Array {
  const bytes = new Uint8Array(128);
  const view = new DataView(bytes.buffer);
  bytes[0] = 0x4d;
  bytes[1] = 0x5a;
  view.setUint32(0x3c, 64, true);
  bytes.set([0x50, 0x45, 0, 0], 64);
  view.setUint16(68, machine, true);
  return bytes;
}

describe("Windows release asset", () => {
  test("reads the x86_64 machine from a valid PE header", () => {
    expect(peMachine(peFixture(WINDOWS_X64_MACHINE))).toBe(WINDOWS_X64_MACHINE);
  });

  test("rejects malformed PE headers", () => {
    expect(() => peMachine(new Uint8Array(63))).toThrow("valid PE image");
    expect(() => peMachine(new Uint8Array(128))).toThrow("valid PE image");

    const invalidOffset = peFixture(WINDOWS_X64_MACHINE);
    new DataView(invalidOffset.buffer).setUint32(0x3c, 126, true);
    expect(() => peMachine(invalidOffset)).toThrow("PE header offset");

    const invalidSignature = peFixture(WINDOWS_X64_MACHINE);
    invalidSignature[64] = 0;
    expect(() => peMachine(invalidSignature)).toThrow("PE signature");
  });
});
