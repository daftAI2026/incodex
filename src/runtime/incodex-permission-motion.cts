// @ts-nocheck
// Adapted from Cavalry-i18n e76175fe09d77b48f8e6306294db758b21325dda,
// macos_permission_handoff.m / ui_review_permission_handoff_runtime.js (MIT).
// Copyright (c) 2026 daftAI. See LICENSE for the permission notice.
function samplePermissionFlight(source, target, seconds) {
  const frequency = 2 * Math.PI / .72;
  const elapsed = Math.max(0,seconds);
  let p = 1 - (1 + frequency * elapsed) * Math.exp(-frequency * elapsed);
  if (1 - p <= .001) p = 1;
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
function runPermissionFlight({source,target,reducedMotion,now,schedule,cancel,render,onComplete}) {
  let disposed=false,handle=null;
  const start=now();
  function frame() {
    if(disposed)return;
    const sample=samplePermissionFlight(source,typeof target === "function" ? target() : target,reducedMotion?3:(now()-start)/1000);
    render(sample);
    if(disposed)return;
    if(sample.progress===1){disposed=true;handle=null;onComplete();return;}
    handle=schedule(frame);
  }
  frame();
  return ()=>{disposed=true;if(handle!==null)cancel(handle);handle=null;};
}
function alignPermissionFrame(frame, scale) {
  if (!Number.isFinite(scale) || scale <= 0) return frame;
  const rounded = value => (value < 0 ? -Math.round(-value * scale) : Math.round(value * scale)) / scale;
  const x = rounded(frame.x), y = rounded(frame.y);
  return { x, y, width: Math.max(0, rounded(frame.x + frame.width) - x), height: Math.max(0, rounded(frame.y + frame.height) - y) };
}
export {samplePermissionFlight,runPermissionFlight,alignPermissionFrame};
