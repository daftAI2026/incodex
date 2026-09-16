import { expect, test } from "bun:test";
import { samplePermissionFlight, runPermissionFlight, alignPermissionFrame } from "./incodex-permission-motion.cts";
const source={x:100,y:100,width:100,height:40,radius:20};
const target={x:600,y:500,width:532,height:112,radius:12};
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

test("flight follows a moved Settings target instead of landing at its stale position", () => {
  let time = 0;
  let next: (() => void) | null = null;
  const liveTarget = { ...target };
  const frames: any[] = [];
  runPermissionFlight({ source, target: () => liveTarget, reducedMotion: false, now: () => time,
    schedule: (callback: () => void) => { next = callback; return 1; }, cancel: () => {},
    render: (frame: any) => frames.push(frame), onComplete: () => {} });
  liveTarget.x = 800;
  liveTarget.y = 600;
  time = 3000;
  next?.();
  expect(frames.at(-1).bounds).toEqual({ x: 800, y: 600, width: 532, height: 112 });
});
