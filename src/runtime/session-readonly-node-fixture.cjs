/**
 * [INPUT]: 依赖编译的真实 safe-home、Node 文件系统与自建 session。
 * [OUTPUT]: 断言只读目录销毁、拒绝分支、链接隔离和父进程重试。
 * [POS]: safe-home.test 的 Node 入口；不触碰真实宿主或用户会话。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const safe = require(process.argv[2]);
const scenario = process.argv[3];
const root = fs.mkdtempSync(path.join(os.tmpdir(), "incodex-readonly-node-"));
const userRoot = path.join(root, ".incodex");
const session = safe.createSessionHome(userRoot, { pid: process.pid });
const owner = JSON.parse(fs.readFileSync(path.join(session.root, "owner.json"), "utf8"));
const snapshot = { pid: owner.pid, processStartIdentity: owner.processStartIdentity };
const expected = { userRoot, sessionId: session.sessionId, ino: session.ino, dev: session.dev };
const generation = path.join(session.chromium, "artifact-sessions/runtime-generations/generation-fixture");
const runtime = path.join(generation, "runtime");
fs.mkdirSync(runtime, { recursive: true });
fs.writeFileSync(path.join(runtime, "fixture"), "owned fixture", { mode: 0o400 });
fs.chmodSync(runtime, 0o500);
fs.chmodSync(generation, 0o500);
const outside = path.join(root, "outside");
fs.mkdirSync(outside);
fs.writeFileSync(path.join(outside, "keep"), "outside fixture", { mode: 0o400 });
fs.chmodSync(outside, 0o500);
const realRm = fs.rmSync;
const realOpen = fs.openSync;

// +---- 只清本夹具，lstat 拒绝把链接当目录。 ----+
function cleanFixture(directory) {
  const stat = fs.lstatSync(directory);
  if (stat.isSymbolicLink() || !stat.isDirectory()) return;
  fs.chmodSync(directory, 0o700);
  for (const name of fs.readdirSync(directory)) cleanFixture(path.join(directory, name));
}

async function run() {
  if (scenario === "owner-mismatch" || scenario === "inode-mismatch") {
    if (scenario === "owner-mismatch") snapshot.pid += 1;
    else expected.ino += 1;
    assert.throws(() => safe.burnSessionHomeWithOwner(session.root, expected, snapshot), /owner|inode/);
    assert.equal(fs.statSync(runtime).mode & 0o777, 0o500);
    assert.equal(fs.existsSync(path.join(session.root, "owner.json")), true);
  } else if (scenario === "live-owner-retained") {
    assert.throws(() => safe.burnSessionHomeWithOwner(session.root, expected, snapshot), /EACCES/);
    assert.equal(fs.statSync(runtime).mode & 0o777, 0o500);
    assert.equal(fs.existsSync(session.root), true);
  } else if (scenario === "sessions-parent-symlink") {
    const sessions = path.join(userRoot, "sessions");
    fs.renameSync(sessions, path.join(root, "moved-sessions"));
    fs.symlinkSync(path.join(root, "moved-sessions"), sessions);
    assert.throws(() => safe.burnSessionHomeWithOwner(session.root, expected, snapshot), /symlink/);
    assert.equal(fs.statSync(runtime).mode & 0o777, 0o500);
    assert.equal(fs.existsSync(path.join(session.root, "owner.json")), true);
  } else if (scenario === "quiesce-refused") {
    assert.equal(await safe.cleanupExitedSession(session, snapshot, {
      userRoot, quiesceSessionHelpers: async () => { throw new Error("unproven helpers"); },
      wait: async () => {},
    }), false);
    assert.equal(fs.statSync(runtime).mode & 0o777, 0o500);
    assert.equal(fs.existsSync(path.join(session.root, "owner.json")), true);
  } else if (scenario === "late-readonly") {
    let recreated = false;
    assert.equal(await safe.cleanupExitedSession(session, snapshot, {
      userRoot, quiesceSessionHelpers: async () => {}, wait: async () => {
        if (!recreated && !fs.existsSync(session.root)) {
          recreated = true;
          fs.mkdirSync(runtime, { recursive: true });
          fs.writeFileSync(path.join(runtime, "fixture"), "late fixture");
          fs.chmodSync(runtime, 0o500);
          fs.chmodSync(generation, 0o500);
        }
      },
    }), true);
    assert.equal(recreated, true);
    assert.equal(fs.existsSync(session.root), false);
  } else if (scenario === "partial-retry") {
    let interrupted = false;
    fs.rmSync = (target, options) => {
      if (target === session.root && !interrupted) {
        interrupted = true;
        fs.unlinkSync(path.join(session.root, "owner.json"));
        throw Object.assign(new Error("fixture interrupted after owner removal"), { code: "EACCES" });
      }
      return realRm(target, options);
    };
    const removed = await safe.cleanupExitedSession(session, snapshot, {
      userRoot, quiesceSessionHelpers: async () => {}, wait: async () => {},
    });
    assert.equal(interrupted, true);
    assert.equal(removed, true);
    assert.equal(fs.existsSync(session.root), false);
  } else if (scenario === "link-swap") {
    let swapped = false;
    let rejectedBeforeRetry = false;
    fs.openSync = (target, ...args) => {
      if (target === runtime && !swapped) {
        swapped = true;
        fs.chmodSync(generation, 0o700);
        fs.renameSync(runtime, path.join(generation, "runtime-original"));
        fs.symlinkSync(outside, runtime);
      }
      return realOpen(target, ...args);
    };
    const removed = await safe.cleanupExitedSession(session, snapshot, {
      userRoot, quiesceSessionHelpers: async () => {}, wait: async () => {
        if (swapped && !rejectedBeforeRetry) {
          rejectedBeforeRetry = true;
          assert.equal(fs.existsSync(path.join(session.root, "owner.json")), true);
          assert.equal(fs.statSync(outside).mode & 0o777, 0o500);
          fs.unlinkSync(runtime);
          fs.renameSync(path.join(generation, "runtime-original"), runtime);
        }
      },
    });
    assert.equal(swapped, true, "must reach the directory-open race");
    assert.equal(rejectedBeforeRetry, true);
    assert.equal(removed, true);
  } else {
    if (scenario === "symlink") {
      fs.chmodSync(generation, 0o700);
      fs.symlinkSync(outside, path.join(generation, "external"));
      fs.symlinkSync(path.join(outside, "keep"), path.join(session.root, "external-file"));
      fs.chmodSync(generation, 0o500);
    }
    assert.equal(await safe.cleanupExitedSession(session, snapshot, {
      userRoot, quiesceSessionHelpers: async () => {}, wait: async () => {},
    }), true);
    assert.equal(fs.existsSync(session.root), false);
  }
  assert.equal(fs.statSync(outside).mode & 0o777, 0o500);
  assert.equal(fs.statSync(path.join(outside, "keep")).mode & 0o777, 0o400);
  assert.equal(fs.readFileSync(path.join(outside, "keep"), "utf8"), "outside fixture");
  console.log(`PASS ${scenario}`);
}

run().catch(error => { console.error(error); process.exitCode = 1; }).finally(() => {
  fs.rmSync = realRm;
  fs.openSync = realOpen;
  cleanFixture(root);
  realRm(root, { recursive: true, force: true });
});
