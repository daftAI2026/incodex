import { expect, test } from "bun:test";
import { createPermissionArrow, HANDOFF_ARROW_SVG } from "./incodex-permission-arrow.cts";
function fixture(reduced = false) {
  let time = 0; let sequence = 0;
  const tasks = new Map<number, {at:number; fn:()=>void}>();
  const listeners: Record<string,()=>void> = {};
  const element = {style:{transform:""},addEventListener:(n:string,f:()=>void)=>{listeners[n]=f},removeEventListener:(n:string)=>{delete listeners[n]}};
  const window = {performance:{now:()=>time},matchMedia:()=>({matches:reduced,addEventListener:()=>{},removeEventListener:()=>{}}),
    setTimeout:(fn:()=>void,ms:number)=>{tasks.set(++sequence,{at:time+ms,fn});return sequence},clearTimeout:(id:number)=>tasks.delete(id),
    requestAnimationFrame:(fn:(now:number)=>void)=>{tasks.set(++sequence,{at:time+16,fn:()=>fn(time)});return sequence},cancelAnimationFrame:(id:number)=>tasks.delete(id)};
  return {element,window,tasks,advance(ms:number){const end=time+ms;for(;;){const next=[...tasks].filter(([,t])=>t.at<=end).sort((a,b)=>a[1].at-b[1].at)[0];if(!next)break;time=next[1].at;tasks.delete(next[0]);next[1].fn()}time=end}};
}
test("reuses Cavalry's filled handoff arrow path",()=>{expect(HANDOFF_ARROW_SVG).toContain('M128,20,232,116');expect(HANDOFF_ARROW_SVG).toContain('viewBox="0 0 256 256"')});
test("starts after 500ms, springs, and fully cancels on close",()=>{const f=fixture();const a=createPermissionArrow(f);a.start();f.advance(499);expect(f.element.style.transform).toBe('scale(1, 1)');f.advance(100);expect(f.element.style.transform).not.toBe('scale(1, 1)');a.dispose();expect(f.tasks.size).toBe(0);expect(f.element.style.transform).toBe('scale(1, 1)')});
test("Reduce Motion keeps the existing arrow still",()=>{const f=fixture(true);const a=createPermissionArrow(f);a.start();f.advance(5000);expect(f.tasks.size).toBe(0);expect(f.element.style.transform).toBe('scale(1, 1)');a.dispose()});
