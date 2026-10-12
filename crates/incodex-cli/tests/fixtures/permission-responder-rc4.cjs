/**
 * [INPUT]: 冻结 Runtime 主入口中的原始 responder 与隔离请求。
 * [OUTPUT]: 复现生产生成器到既有 UUID 读端的真实字节交接。
 * [POS]: 只用于跨语言回归；不进入产品或触碰真实安装。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
// 固定来源：RC4 main SHA256 7f66adbc7dada2929a105c7e050176707c7160c1d56bcfbd42f3f0e09612ef71。
module.exports = (()=>{const t={exports:{}};const n=t.exports;"use strict";Object.defineProperty(n,"__esModule",{value:true});n.createPermissionProbeResponder=u;n.readScreenPermission=a;const i=require("node:fs");const o=require("node:path");const s="/Applications/ChatGPT.app";const l=/^[a-f0-9]{8}-[a-f0-9]{4}-[1-5][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/i;const f="permission-probe-request.json";const m="permission-probe-response.json";const g=8*1024;const S=["schemaVersion","nonce","installId","requestId","appPath","pid","processStartIdentity","requestedAtMs"].sort();function a(t){let n="unknown";try{const i=t?.getMediaAccessStatus?.("screen");if(["granted","denied","not-determined","restricted"].includes(i))n=i}catch{}return{rawStatus:n,status:n==="granted"?"granted":n==="unknown"?"unknown":"not-granted"}}function u(t={}){const{root:n,installId:I,appPath:E,pid:k,processStartIdentity:_,systemPreferences:O}=t;if((t.platform||process.platform)!=="darwin"||t.incognito||E!==s||!l.test(I||"")||!o.isAbsolute(n||"")||!Number.isSafeInteger(k)||k<=0||typeof _!=="string"||!_)return null;const C=process.getuid?.();if(!Number.isInteger(C))return null;const R=o.join(n,"transactions",I);const L=o.join(R,f);const M=o.join(R,m);const x=o.join(R,"accessibility-setup.json");const N=t.now||Date.now;let W=false,T=null,D=null;function q(){return[n,o.join(n,"transactions"),R].map(t=>{const n=i.lstatSync(t);if(!n.isDirectory()||n.isSymbolicLink()||n.uid!==C||n.mode&63)throw new Error("unsafe probe directory");return`${n.dev}:${n.ino}`}).join("/")}function P(t){const n=i.openSync(t,i.constants.O_RDONLY|i.constants.O_NOFOLLOW|i.constants.O_NONBLOCK);try{const t=i.fstatSync(n);if(!t.isFile()||t.uid!==C||t.mode&63||t.size>g)throw new Error("unsafe probe file");const o=Buffer.alloc(g+1);const s=i.readSync(n,o,0,o.length,0);if(s>g)throw new Error("oversized probe file");return o.subarray(0,s).toString("utf8")}finally{i.closeSync(n)}}function A(t){const n=JSON.parse(P(x));return n.schemaVersion===1&&n.installId===I&&n.appPath===s&&n.presentationOwner==="cli"&&n.requestId===t.requestId&&["pending","awaiting-user"].includes(n.state)}function v(){if(W)return;let n=null;try{const f=q();const m=P(L);const g=JSON.parse(m);if(!g||Object.keys(g).sort().join()!==S.join()||g.schemaVersion!==1||!/^[a-f0-9]{32}$/.test(g.nonce||"")||g.nonce===D||g.installId!==I||!l.test(g.requestId||"")||g.appPath!==s||g.pid!==k||g.processStartIdentity!==_||!Number.isSafeInteger(g.requestedAtMs)||g.requestedAtMs>N()||N()-g.requestedAtMs>5e3||!A(g))return;const E=a(O);if(P(L)!==m||!A(g)||q()!==f||W)return;const C={...g,...E,observedAtMs:N(),versions:{electron:String(t.versions?.electron||"").slice(0,64),chrome:String(t.versions?.chrome||"").slice(0,64)}};try{P(M)}catch(t){if(t.code!=="ENOENT")return}n=o.join(R,`.permission-probe-response.${k}.${g.nonce}.tmp`);const x=i.openSync(n,"wx",384);try{i.writeFileSync(x,`${JSON.stringify(C)}\n`);i.fsyncSync(x)}finally{i.closeSync(x)}if(P(L)!==m||!A(g)||q()!==f||W)return;i.renameSync(n,M);n=null;D=g.nonce}catch{}finally{if(n){try{i.unlinkSync(n)}catch{}}}}if(t.watch!==false){try{q();T=i.watch(R,{persistent:false},(t,n)=>{if(n===null||String(n)===f)v()});T.on("error",()=>{T?.close();T=null})}catch{return null}}v();return{respond:v,dispose(){W=true;T?.close();T=null}}}return t.exports})();

if (require.main === module) {
  const assert = require("node:assert/strict");
  const fs = require("node:fs");
  const path = require("node:path");
  const root = fs.mkdtempSync(path.join(process.argv[3], "wire-"));
  fs.chmodSync(root, 0o700);
  const installId = process.argv[4];
  const directory = path.join(root, "transactions", installId);
  fs.mkdirSync(directory, {recursive: true, mode: 0o700});
  fs.chmodSync(path.dirname(directory), 0o700);
  let observed = "unknown";
  const responder = module.exports.createPermissionProbeResponder({
    root, installId, appPath: "/Applications/ChatGPT.app", pid: process.pid,
    processStartIdentity: "isolated-wire", platform: "darwin", watch: false,
    now: () => 2000, systemPreferences: {getMediaAccessStatus: () => observed},
  });
  assert.ok(responder, "frozen responder must start");
  const observations = [];
  function attempt(requestId, nonce, expected) {
    const identity = {installId, requestId, appPath: "/Applications/ChatGPT.app"};
    const request = {schemaVersion: 1, ...identity, nonce, pid: process.pid,
      processStartIdentity: "isolated-wire", requestedAtMs: 2000};
    fs.writeFileSync(path.join(directory, "accessibility-setup.json"), JSON.stringify({
      schemaVersion: 1, ...identity, presentationOwner: "cli", state: "pending",
    }), {mode: 0o600});
    fs.writeFileSync(path.join(directory, "permission-probe-request.json"), JSON.stringify(request), {mode: 0o600});
    const response = path.join(directory, "permission-probe-response.json");
    fs.rmSync(response, {force: true}); responder.respond();
    assert.equal(fs.existsSync(response), expected, "production request format must interoperate");
    if (!expected) return;
    const value = JSON.parse(fs.readFileSync(response, "utf8"));
    assert.equal(value.requestId, requestId); assert.equal(value.installId, installId);
    assert.equal(value.nonce, nonce); assert.equal(value.rawStatus, observed);
    const status = observed === "granted" ? "granted" : observed === "unknown" ? "unknown" : "not-granted";
    assert.equal(value.status, status); observations.push(status);
  }
  try {
    // 正负控制证明夹具确实执行原始 gate，而不是永远成功。
    attempt("22222222-2222-4222-8222-222222222222", "a".repeat(32), true);
    attempt("2".repeat(32), "b".repeat(32), false);
    for (const [index, state] of ["granted", "denied", "unknown"].entries()) {
      observed = state; attempt(process.argv[2], String(index+1).repeat(32), true);
    }
    process.stdout.write(JSON.stringify({result: "PASS", observations}));
  } finally {responder.dispose(); fs.rmSync(root, {recursive: true});}
}
