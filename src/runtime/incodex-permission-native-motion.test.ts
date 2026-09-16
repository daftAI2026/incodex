import { expect, test } from "bun:test";
import { runNativePermissionHandoff } from "./incodex-permission-native-motion.cts";

function harness(reducedMotion = false) {
  let time = 0;
  let closed = false;
  const pending = new Map<number, () => void>();
  let id = 0;
  const frames: any[] = [];
  let disposed = 0;
  let created = 0;
  const flight = runNativePermissionHandoff({
    source: { frame: { origin: { x: 10, y: 400 }, size: { width: 80, height: 28 } }, image: {}, radius: 14 },
    target: { frame: { origin: { x: 300, y: 20 }, size: { width: 532, height: 112 } }, view: {}, panel: {}, radius: 12 },
    reducedMotion,
    isClosed: () => closed,
    now: () => time,
    schedule: (callback: () => void) => { pending.set(++id, callback); return id; },
    cancel: (key: number) => pending.delete(key),
    createReplicants: () => {
      created++;
      return { render: (frame: any) => frames.push(frame), dispose: () => disposed++ };
    },
  });
  return { flight, frames, pending, created: () => created, disposed: () => disposed,
    closeHost() { closed = true; },
    advance(ms: number) { time += ms; const tasks = [...pending.values()]; pending.clear(); tasks.forEach(task => task()); } };
}

test("native flight preserves AppKit screen coordinates at both endpoints", async () => {
  const h = harness();
  expect(h.frames[0].bounds).toEqual({ x: 10, y: 400, width: 80, height: 28 });
  h.advance(3000);
  await h.flight.finished;
  expect(h.frames.at(-1).bounds).toEqual({ x: 300, y: 20, width: 532, height: 112 });
  expect(h.frames.at(-1).targetOpacity).toBe(1);
  expect(h.disposed()).toBe(1);
  expect(h.pending.size).toBe(0);
});

test("closing during native flight reaps panels and settles without late frames", async () => {
  const h = harness();
  h.flight.dispose();
  h.flight.dispose();
  await h.flight.finished;
  const count = h.frames.length;
  h.advance(3000);
  expect(h.frames).toHaveLength(count);
  expect(h.pending.size).toBe(0);
  expect(h.disposed()).toBe(1);
});

test("Reduce Motion avoids creating any snapshot panels", async () => {
  const h = harness(true);
  await h.flight.finished;
  expect(h.created()).toBe(0);
  expect(h.pending.size).toBe(0);
});

test("host closure observed inside a frame leaves no timer behind", async () => {
  const h = harness();
  h.closeHost();
  h.advance(16);
  await h.flight.finished;
  expect(h.pending.size).toBe(0);
  expect(h.disposed()).toBe(1);
});
