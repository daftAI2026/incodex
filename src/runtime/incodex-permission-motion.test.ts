import { expect, test } from "bun:test";
import { samplePermissionFlight, runPermissionFlight, alignPermissionFrame } from "./incodex-permission-motion.cts";
import * as motion from "./incodex-permission-motion.cts";
const source={x:100,y:100,width:100,height:40,radius:20};
const target={x:600,y:500,width:532,height:112,radius:12};

test("permission spring advances persistent velocity-Verlet state at 240Hz", () => {
  const state = motion.createPermissionSpring();
  Object.assign(state, { value: 0, velocity: 0, force: 0, time: 0 });
  motion.advancePermissionSpring(state, 1 / 240);
  expect(state.time).toBe(1 / 240);
  expect(state.value).toBe(0);
  expect(state.velocity).toBeCloseTo(0.15865490613891076, 13);
  expect(state.force).toBeCloseTo(76.15435494667716, 12);
  motion.advancePermissionSpring(state, 2 / 240);
  expect(state.value).toBeCloseTo(0.0013221242178242563, 14);
  expect(state.velocity).toBeCloseTo(0.4642172465623554, 13);
  expect(state.force).toBeCloseTo(70.51556845657626, 12);
});

test("spring idle requires both squared velocity/force and relative target gates", () => {
  const state = { value: 1, target: 1, velocity: 0, force: 0 };
  expect(motion.permissionSpringIsIdle(state)).toBe(true);
  expect(motion.permissionSpringIsIdle({ ...state, velocity: .061 })).toBe(false);
  expect(motion.permissionSpringIsIdle({ ...state, force: .061 })).toBe(false);
  expect(motion.permissionSpringIsIdle({ ...state, value: .98 })).toBe(false);
  expect(motion.permissionSpringIsIdle({ ...state, value: .995 })).toBe(true);
  expect(motion.permissionSpringIsIdle({ ...state, value: 1, target: 0 })).toBe(true);
  expect(motion.permissionSpringIsIdle({ ...state, value: Number.NaN })).toBe(true);
  expect(motion.permissionSpringIsIdle({ ...state, target: Number.NaN })).toBe(true);
  // Swift.max preserves the first unordered operand; Math.max is not a substitute.
  expect(motion.permissionSpringIsIdle({ ...state, velocity: 1, force: Number.NaN })).toBe(false);
});

test("spring caps stale catch-up time rather than snapping after a suspended frame", () => {
  const state = motion.createPermissionSpring();
  const progress = motion.advancePermissionSpring(state, 3);
  expect(progress).toBeLessThan(.02);
  expect(state.time).toBeGreaterThanOrEqual(3);
  expect(state.time).toBeLessThan(3 + 1 / 240);
  const prior = state.value;
  motion.advancePermissionSpring(state, 2);
  expect(state.value).toBe(prior);
});

test("scheduled flight does not complete when its next frame arrives three seconds late", () => {
  let time = 0;
  let pending: (() => void) | undefined;
  let completed = 0;
  const frames: any[] = [];
  runPermissionFlight({ source, target, reducedMotion: false, now: () => time,
    schedule: (callback: () => void) => { pending = callback; return 1; }, cancel: () => {},
    render: (frame: any) => frames.push(frame), onComplete: () => completed++ });
  time = 3000;
  pending?.();
  expect(completed).toBe(0);
  expect(frames.at(-1).progress).toBeLessThan(.02);
});

test("display-link frame source drives relative timestamps and stops after dispose", () => {
  let emit: ((timestamp: number) => void) | undefined;
  let starts = 0;
  let stops = 0;
  let timerSchedules = 0;
  const frames: any[] = [];
  const frameSource = {
    start(callback: (timestamp: number) => void) {
      starts++;
      emit = callback;
      return true;
    },
    stop() { stops++; },
  };
  const stop = runPermissionFlight({ source, target, reducedMotion: false,
    frameSource, now: () => 0,
    schedule: () => { timerSchedules++; return 1; }, cancel: () => {},
    render: frame => frames.push(frame), onComplete: () => {} });

  expect(starts).toBe(1);
  expect(timerSchedules).toBe(0);
  expect(frames).toHaveLength(1);
  emit?.(100);
  expect(frames).toHaveLength(2);
  expect(frames.at(-1).progress).toBe(0);
  emit?.(100.25);
  expect(frames.at(-1).progress).toBeGreaterThan(0);
  stop();
  expect(stops).toBe(1);
  const count = frames.length;
  emit?.(100.5);
  expect(frames).toHaveLength(count);
});

test("a display source that declines falls back to timer frames until completion", () => {
  let timerCallback: (() => void) | undefined;
  let schedules = 0;
  const frameSource = { start: () => false, stop: () => { throw new Error("must not stop a declined source"); } };
  const stop = runPermissionFlight({ source, target, reducedMotion: false, frameSource, now: () => 0,
    schedule: callback => { schedules++; timerCallback = callback; return schedules; }, cancel: () => {},
    render: () => {}, onComplete: () => {} });
  expect(schedules).toBe(1);
  timerCallback?.();
  expect(schedules).toBe(2);
  stop();
});

test("a completed display-link flight stops its source before completion", () => {
  let emit: ((timestamp: number) => void) | undefined;
  let stopped = 0;
  let complete = 0;
  const frameSource = {
    start(callback: (timestamp: number) => void) { emit = callback; return true; },
    stop() { stopped++; },
  };
  runPermissionFlight({ source, target, reducedMotion: false, frameSource, now: () => 0,
    schedule: () => { throw new Error("display source should own scheduling"); }, cancel: () => {},
    render: () => {}, onComplete: () => { complete++; } });
  emit?.(10);
  for (let timestamp = 10 + 1 / 60; timestamp <= 12.1 && complete === 0; timestamp += 1 / 60) emit?.(timestamp);
  expect(complete).toBe(1);
  expect(stopped).toBe(1);
});

test("invalid or backwards display timestamps never switch back to the wall clock", () => {
  let emit: ((timestamp: number) => void) | undefined;
  let frames: any[] = [];
  const frameSource = {
    start(callback: (timestamp: number) => void) { emit = callback; return true; },
    stop() {},
  };
  runPermissionFlight({ source, target, reducedMotion: false, frameSource, now: () => 99_000,
    schedule: () => { throw new Error("display source should own scheduling"); }, cancel: () => {},
    render: frame => frames.push(frame), onComplete: () => {} });
  emit?.(4);
  expect(frames).toHaveLength(2);
  emit?.(Number.NaN);
  expect(frames).toHaveLength(2);
  emit?.(3);
  expect(frames.at(-1).progress).toBe(0);
  emit?.(4.25);
  expect(frames.at(-1).progress).toBeGreaterThan(0);
});

test("Cavalry handoff begins at source and settles exactly at target",()=>{
  expect(samplePermissionFlight(source,target,0).bounds).toEqual({x:100,y:100,width:100,height:40});
  const final=samplePermissionFlight(source,target,3);
  expect(final.bounds).toEqual({x:600,y:500,width:532,height:112});
  expect(final.progress).toBe(1);expect(final.sourceOpacity).toBe(0);expect(final.targetBlur).toBe(0);
});
test("handoff uses complementary opacity and blur rather than abruptly swapping images",()=>{
  const mid=samplePermissionFlight(source,target,.2);
  expect(mid.progress).toBeGreaterThan(0);expect(mid.progress).toBeLessThan(1);
  expect(mid.sourceOpacity+mid.targetOpacity).toBeCloseTo(1);
  expect(mid.sourceBlur+mid.targetBlur).toBeCloseTo(12);
});
test("closing cancels queued frames and never completes into a disposed guide",()=>{
  let time=0;let pending:any=null;let completed=0;const frames:any[]=[];
  const stop=runPermissionFlight({source,target,reducedMotion:false,now:()=>time,schedule:(fn:any)=>{pending=fn;return 1},cancel:()=>{pending=null},render:(f:any)=>frames.push(f),onComplete:()=>completed++});
  time=100;pending();expect(frames.length).toBeGreaterThan(1);stop();expect(pending).toBe(null);expect(completed).toBe(0);
});
test("Reduce Motion presents destination without scheduling flight",()=>{
  const frames:any[]=[];let complete=0;
  runPermissionFlight({source,target,reducedMotion:true,now:()=>0,schedule:()=>{throw Error('must not animate')},cancel:()=>{},render:(f:any)=>frames.push(f),onComplete:()=>complete++});
  expect(frames).toHaveLength(1);expect(frames[0].progress).toBe(1);expect(complete).toBe(1);
});

test("flight retains fractional points until each display aligns its backing pixels", () => {
  const from = { x: 100.25, y: 100.5, width: 46.5, height: 20.25, radius: 10 };
  const to = { x: -601.5, y: 35.5, width: 451.5, height: 44.5, radius: 8 };
  expect(samplePermissionFlight(from, to, 0).bounds).toEqual({ x: 100.25, y: 100.5, width: 46.5, height: 20.25 });
  expect(samplePermissionFlight(from, to, 3).bounds).toEqual({ x: -601.5, y: 35.5, width: 451.5, height: 44.5 });
});


test("native pixel alignment rounds rectangle edges with C round semantics on negative displays", () => {
  expect(alignPermissionFrame({ x: .25, y: -1.25, width: .5, height: .75 }, 2))
    .toEqual({ x: .5, y: -1.5, width: .5, height: 1 });
  expect(alignPermissionFrame({ x: .3, y: .3, width: .3, height: .3 }, 1))
    .toEqual({ x: 0, y: 0, width: 1, height: 1 });
});

test("flight preserves the screen-space quadratic arc and centered size interpolation", () => {
  // An independent bottom-up screen-space construction checks the top-down
  // motion adapter in both directions. The curve passes 50pt above the higher
  // endpoint at p=.5; this point is not necessarily its mathematical apex.
  const fixtures = [
    [{ x: 100.25, y: 400.5, width: 518, height: 80, radius: 24 }, { x: 700.5, y: 200.25, width: 531, height: 110, radius: 14 }],
    [{ x: -1600.5, y: -150.25, width: 518, height: 80, radius: 24 }, { x: -900.25, y: 630.5, width: 531, height: 126, radius: 14 }],
    [{ x: 500, y: 300, width: 518, height: 80, radius: 24 }, { x: 200, y: 285, width: 531, height: 110, radius: 14 }],
  ];
  const topDown = (r: typeof source) => ({ ...r, y: -r.y - r.height });
  for (const pair of fixtures) for (const [from, to] of [pair, [...pair].reverse()]) {
    const start = { x: from.x + from.width / 2, y: from.y + from.height / 2 };
    const end = { x: to.x + to.width / 2, y: to.y + to.height / 2 };
    const control = {
      x: 2 * ((start.x + end.x) / 2 - start.x / 4 - end.x / 4),
      y: 2 * (Math.max(start.y, end.y) + 50 - start.y / 4 - end.y / 4),
    };
    for (const p of [0, .1, .25, .5, .75, .9, 1]) {
      const q = 1 - p;
      const center = { x: q * q * start.x + 2 * q * p * control.x + p * p * end.x,
        y: q * q * start.y + 2 * q * p * control.y + p * p * end.y };
      const width = from.width + p * (to.width - from.width);
      const height = from.height + p * (to.height - from.height);
      const actual = motion.samplePermissionFlightAtProgress(topDown(from), topDown(to), p);
      expect(actual.bounds.x).toBeCloseTo(center.x - width / 2, 9);
      expect(-actual.bounds.y - actual.bounds.height).toBeCloseTo(center.y - height / 2, 9);
      expect(actual.bounds.width).toBeCloseTo(width, 9);
      expect(actual.bounds.height).toBeCloseTo(height, 9);
      expect(actual.cornerRadius).toBeCloseTo(from.radius + p * (to.radius - from.radius), 9);
    }
  }
});

test("flight follows a moved Settings target instead of landing at its stale position", () => {
  let time = 0;
  const pending: { next: (() => void) | null } = { next: null };
  const liveTarget = { ...target };
  const frames: any[] = [];
  runPermissionFlight({ source, target: () => liveTarget, reducedMotion: false, now: () => time,
    schedule: (callback: () => void) => { pending.next = callback; return 1; }, cancel: () => {},
    render: (frame: any) => frames.push(frame), onComplete: () => {} });
  liveTarget.x = 800;
  liveTarget.y = 600;
  for (let tick = 0; tick < 180; tick++) { time += 1000 / 60; pending.next?.(); }
  expect(frames.at(-1).bounds).toEqual({ x: 800, y: 600, width: 532, height: 112 });
});

test("reverse flight starts at the helper and returns to the original source", () => {
  let time = 0;
  const pending: { next: (() => void) | null } = { next: null };
  const frames: any[] = [];
  runPermissionFlight({ source, target, reverse: true, reducedMotion: false, now: () => time,
    schedule: (callback: () => void) => { pending.next = callback; return 1; }, cancel: () => {},
    render: (frame: any) => frames.push(frame), onComplete: () => {} });

  expect(frames[0].progress).toBe(1);
  expect(frames[0].bounds).toEqual({ x: 600, y: 500, width: 532, height: 112 });
  expect(frames[0].sourceOpacity).toBe(0);
  expect(frames[0].targetOpacity).toBe(1);
  for (let tick = 0; tick < 180; tick++) { time += 1000 / 60; pending.next?.(); }
  expect(frames.at(-1).progress).toBe(0);
  expect(frames.at(-1).bounds).toEqual({ x: 100, y: 100, width: 100, height: 40 });
  expect(frames.at(-1).sourceOpacity).toBe(1);
  expect(frames.at(-1).targetOpacity).toBe(0);
});

test("reverse Reduce Motion cleans up without presenting a replacement frame", () => {
  const frames: any[] = [];
  let complete = 0;
  runPermissionFlight({ source, target, reverse: true, reducedMotion: true, now: () => 0,
    schedule: () => { throw Error("must not animate"); }, cancel: () => {},
    render: (frame: any) => frames.push(frame), onComplete: () => complete++ });
  expect(frames).toHaveLength(0);
  expect(complete).toBe(1);
});
