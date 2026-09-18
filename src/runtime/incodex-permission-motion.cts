// @ts-nocheck
// Adapted from Cavalry-i18n e76175fe09d77b48f8e6306294db758b21325dda,
// macos_permission_handoff.m / ui_review_permission_handoff_runtime.js (MIT).
// Copyright (c) 2026 daftAI. See LICENSE for the permission notice.
function createPermissionSpring() {
  return { value: 0, target: 1, velocity: 0, force: 0, time: 0, settled: false };
}
function permissionSpringIsIdle({ value, target, velocity, force }) {
  const velocitySquared = velocity * velocity, forceSquared = force * force;
  // Preserve ordered comparisons, including the first-operand NaN behavior.
  const metric = velocitySquared < forceSquared ? forceSquared : velocitySquared;
  if (metric > .06 * .06) return false;
  const tolerance = (target * .01) ** 2, distance = (target - value) ** 2;
  return !(tolerance > 0) || !(distance > tolerance);
}
function advancePermissionSpring(state, targetTime) {
  if (state.settled || !Number.isFinite(targetTime) || targetTime <= state.time) return state.value;
  const dt = 1 / 240, halfStep = dt / 2;
  const stiffness = Math.min((2 * Math.PI / .72) ** 2, 28800);
  const drag = 2 * Math.sqrt(stiffness);
  // A suspended display clock resumes with one frame, not seconds of catch-up.
  if (targetTime - state.time > 1) state.time = targetTime - 1 / 60;
  while (state.time < targetTime) {
    const halfVelocity = state.velocity + state.force * halfStep;
    state.value += halfVelocity * dt;
    state.force = stiffness * (state.target - state.value) - drag * halfVelocity;
    state.velocity = halfVelocity + state.force * halfStep;
    state.time += dt;
  }
  if (permissionSpringIsIdle(state)) {
    state.value = state.target;
    state.settled = true;
  }
  return state.value;
}
function springPermissionProgress(seconds) {
  const state = createPermissionSpring();
  const elapsed = Number.isFinite(seconds) ? Math.max(0, seconds) : 0;
  // Stateless samples describe an uninterrupted display timeline. The live
  // scheduler below instead retains state and applies the stall catch-up rule.
  for (let time = 1 / 60; time < elapsed && !state.settled; time += 1 / 60) {
    advancePermissionSpring(state, time);
  }
  return advancePermissionSpring(state, elapsed);
}
function samplePermissionFlightAtProgress(source, target, progress) {
  const p = Math.max(0, Math.min(1, Number(progress)));
  const lerp=(a,b)=>a+(b-a)*p;
  const from={x:source.x+source.width/2,y:source.y+source.height/2};
  const to={x:target.x+target.width/2,y:target.y+target.height/2};
  const apex=Math.min(from.y,to.y)-50;
  const controlY=2*apex-(from.y+to.y)/2;
  const centerY=(1-p)*(1-p)*from.y+2*(1-p)*p*controlY+p*p*to.y;
  const width=lerp(source.width,target.width),height=lerp(source.height,target.height);
  return {progress:p,bounds:{x:lerp(from.x,to.x)-width/2,y:centerY-height/2,width,height},
    sourceOpacity:1-p,targetOpacity:p,sourceBlur:12*p,targetBlur:12*(1-p),cornerRadius:lerp(source.radius,target.radius)};
}
function samplePermissionFlight(source, target, seconds) {
  return samplePermissionFlightAtProgress(source, target, springPermissionProgress(seconds));
}
function runPermissionFlight({source,target,reducedMotion,reverse=false,now,schedule,cancel,frameSource=null,render,onComplete}) {
  let disposed=false,handle=null,frameSourceStarted=false,useTimer=false,displayOrigin=null;
  const start=now();
  const spring=createPermissionSpring();
  function stopFrameSource() {
    if (!frameSourceStarted) return;
    frameSourceStarted=false;
    frameSource.stop?.();
  }
  function complete() {
    disposed=true;handle=null;
    let firstError;
    try { stopFrameSource(); } catch (error) { firstError=error; }
    try { onComplete(); } catch (error) { if (!firstError) firstError=error; }
    if (firstError) throw firstError;
  }
  function frame(displayTimestamp) {
    if(disposed)return;
    if (reducedMotion && reverse) { complete(); return; }
    if (displayTimestamp !== undefined && !Number.isFinite(displayTimestamp)) return;
    const destination=typeof target === "function" ? target() : target;
    let elapsed=(now()-start)/1000;
    if (typeof displayTimestamp === "number") {
      if (displayOrigin === null) displayOrigin=displayTimestamp;
      elapsed=Math.max(0,displayTimestamp-displayOrigin);
    }
    const forward=reducedMotion ? 1 : advancePermissionSpring(spring,elapsed);
    const sample=samplePermissionFlightAtProgress(source,destination,reverse ? 1-forward : forward);
    render(sample);
    if(disposed)return;
    if((reverse ? sample.progress===0 : sample.progress===1)){
      complete();return;
    }
    if (frameSourceStarted || (frameSource && !useTimer)) return;
    handle=schedule(frame);
  }
  function scheduleNext() {
    if (frameSource && !frameSourceStarted && !useTimer) {
      frameSourceStarted=true;
      let started=false;
      try { started=Boolean(frameSource.start(frame)); }
      catch (error) { frameSourceStarted=false; throw error; }
      if (started) return;
      frameSourceStarted=false;
      useTimer=true;
    }
    handle=schedule(frame);
  }
  frame();
  if (!disposed) {
    // The first frame establishes the initial state before the native window
    // is ordered front. The display source is started only after that render.
    if (frameSource) scheduleNext();
  }
  return ()=>{
    disposed=true;
    if(handle!==null)cancel(handle);
    handle=null;
    stopFrameSource();
  };
}
function alignPermissionFrame(frame, scale) {
  if (!Number.isFinite(scale) || scale <= 0) return frame;
  const rounded = value => (value < 0 ? -Math.round(-value * scale) : Math.round(value * scale)) / scale;
  const x = rounded(frame.x), y = rounded(frame.y);
  return { x, y, width: Math.max(0, rounded(frame.x + frame.width) - x), height: Math.max(0, rounded(frame.y + frame.height) - y) };
}
export {createPermissionSpring,advancePermissionSpring,permissionSpringIsIdle,samplePermissionFlight,samplePermissionFlightAtProgress,runPermissionFlight,alignPermissionFrame};
