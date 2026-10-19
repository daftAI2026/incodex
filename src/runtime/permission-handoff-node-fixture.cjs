/**
 * [INPUT]: 依赖主 Runtime 源码、私有 marker 与 Node 的真实目录 watcher。
 * [OUTPUT]: 执行原子 marker 发布、权限续接与 watcher 关闭断言。
 * [POS]: permission-handoff 测试的 Node 子进程入口，不启动宿主或读取 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const vm = require("node:vm");

const APP = "/Applications/ChatGPT.app";
const INSTALL = "11111111-1111-4111-8111-111111111111";
const REQUEST_ID = INSTALL;
const main = fs.readFileSync(process.env.INCODEX_PERMISSION_HANDOFF_MAIN_SOURCE, "utf8");
const begin = main.indexOf("function accessibilityCurrentUid()");
const end = main.indexOf("\nfunction sessionBurnExpectation(", begin);
assert(begin >= 0 && end > begin, "native permission controller source was not found");

const root = fs.mkdtempSync(path.join(os.tmpdir(), "incodex-marker-watch-node-"));
const directory = path.join(root, "transactions", INSTALL);
const requestPath = path.join(directory, "accessibility-setup.json");
fs.mkdirSync(directory, { recursive: true, mode: 0o700 });
const marker = { schemaVersion: 1, installId: INSTALL, appPath: APP, requestedAtMs: 1_700_000_000_000,
  requestId: REQUEST_ID, state: "pending", presentationOwner: "cli" };
fs.writeFileSync(requestPath, JSON.stringify(marker) + "\n", { mode: 0o600 });

let watcherCount = 0;
let watcherCloseCount = 0;
const realWatch = fs.watch;
const fileSystem = { ...fs, watch(...args) {
  watcherCount++;
  const watcher = realWatch.apply(fs, args);
  const close = watcher.close.bind(watcher);
  watcher.close = () => { watcherCloseCount++; return close(); };
  return watcher;
} };
const taskCalls = [];
let announceTaskStarted;
let releaseTask;
const taskStarted = new Promise(resolve => { announceTaskStarted = resolve; });
const taskResult = new Promise(resolve => { releaseTask = resolve; });
const options = { platform: "darwin", appPath: APP, installId: INSTALL, requestPath,
  fs: fileSystem, isIncognito: false,
  startPermissionCheck(request) { taskCalls.push(request); announceTaskStarted(request); return taskResult; } };
const controller = vm.runInNewContext(main.slice(begin, end) + ";createNativePermissionSetupController(options)", {
  options,
  process: { platform: "darwin", getuid: () => typeof process.getuid === "function" ? process.getuid() : null },
  path, fs: fileSystem, Buffer, setTimeout, clearTimeout,
  ACCESSIBILITY_MARKER_NAME: "accessibility-setup.json",
  ACCESSIBILITY_MAX_MARKER_BYTES: 8 * 1024,
  ACCESSIBILITY_PATH: APP,
  accessibilityInstallIdIsSafe: id => /^[A-Za-z0-9._-]{1,128}$/.test(id),
  accessibilityPathIsDefaultApp: app => path.resolve(app) === APP,
});

function publish(value) {
  const temporary = requestPath + "." + Date.now() + "." + Math.random().toString(16).slice(2) + ".tmp";
  fs.writeFileSync(temporary, JSON.stringify(value) + "\n", { mode: 0o600 });
  fs.renameSync(temporary, requestPath);
}

function delay(ms) { return new Promise(resolve => setTimeout(resolve, ms)); }

async function run() {
  try {
    assert.equal(process.versions.bun, undefined, "real fs.watch fixture must run under Node");
    const firstRun = await controller.run();
    assert.equal(firstRun.state, "pending");
    assert.equal(watcherCount, 1);

    publish({ ...marker, resumeReason: "post-update", updatedAtMs: 1_700_000_000_100 });
    let deadline;
    let started;
    try {
      started = await Promise.race([
        taskStarted,
        new Promise((_, reject) => { deadline = setTimeout(() => reject(new Error("marker edge was lost")), 1500); }),
      ]);
    } finally {
      clearTimeout(deadline);
    }
    assert.equal(started && started.resumeRequestId, REQUEST_ID);

    publish({ ...marker, resumeReason: "post-update", updatedAtMs: 1_700_000_000_100 });
    await delay(50);
    assert.equal(taskCalls.length, 1);

    publish({ ...marker, state: "deferred", updatedAtMs: 1_700_000_000_200 });
    await delay(50);
    assert.equal(taskCalls.length, 1);

    controller.dispose();
    assert.equal(watcherCloseCount, 1);
    releaseTask({ ok: true, state: "native-owned" });
    publish({ ...marker, resumeReason: "post-update", updatedAtMs: 1_700_000_000_200 });
    await delay(50);
    assert.equal(taskCalls.length, 1);
    process.stdout.write(JSON.stringify({ engine: "node", bunVersion: null, watcherCount, watcherCloseCount,
      taskCalls: taskCalls.length, request: taskCalls[0] }) + "\n");
  } finally {
    controller.dispose();
    releaseTask({ ok: true, state: "native-owned" });
    fs.rmSync(root, { recursive: true, force: true });
  }
}

run().catch(error => {
  process.stderr.write(String(error.stack || error) + "\n");
  process.exitCode = 1;
});
