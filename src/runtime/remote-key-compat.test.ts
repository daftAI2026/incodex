/**
 * [INPUT]: 依赖 adapter 清单、临时 native fixture 与真实 Node/Worker 进程。
 * [OUTPUT]: 验证旧回退、libuv 并发、EOF 后审批、环境取消与空闲退出。
 * [POS]: 密钥兼容与资产回归；不读用户 Keychain 或宿主密钥。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { spawn, spawnSync } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "bun:test";
import { runInNewContext } from "node:vm";
import { deviceKeyAdapterSources } from "../../scripts/build-device-key-adapter.ts";
import { installRemoteKeyCompatibility } from "./incodex-remote-key-compat.cts";

function buildDeviceNapiFixture(directory: string, addonFlags: string[] = []): string {
  const source = join(import.meta.dir, "../../native/macos/device-key-bridge-napi.test.m");
  const addon = join(directory, "fixture.node"), helper = join(directory, "fixture-helper");
  const compile = (output: string, flags: string[]) => {
    const result = spawnSync("xcrun", ["clang", "-fobjc-arc", "-Wno-deprecated-declarations", "-framework", "Foundation", "-framework", "Security", `-DINCODEX_DEVICE_NAPI_FIXTURE_HOME=\"${directory}\"`, ...flags, source, "-o", output], { encoding: "utf8" });
    expect(result.stderr).not.toContain("error:");
    expect(result.status).toBe(0);
  };
  compile(addon, ["-shared", "-undefined", "dynamic_lookup", ...addonFlags]);
  compile(helper, ["-DINCODEX_DEVICE_BRIDGE_HELPER_FIXTURE"]);
  const bytes = readFileSync(helper), digest = createHash("sha256").update(bytes).digest("hex");
  const helperRelease = join(directory, ".incodex/helpers/macos-device-key", digest);
  mkdirSync(helperRelease, { recursive: true, mode: 0o700 });
  const helperPath = join(helperRelease, "incodex-device-key-helper");
  writeFileSync(helperPath, bytes, { mode: 0o700 });
  const namespace = join(directory, ".incodex/macos-device-key");
  mkdirSync(namespace, { recursive: true, mode: 0o700 });
  writeFileSync(join(namespace, "registration.json"), JSON.stringify({ schemaVersion: 1, protocolVersion: 1, helperSha256: digest }), { mode: 0o600 });
  for (const part of [".incodex", ".incodex/helpers", ".incodex/helpers/macos-device-key", ".incodex/macos-device-key"]) chmodSync(join(directory, part), 0o700);
  return addon;
}

test("failure selector diagnostics retain the synthetic signing target", () => {
  const source = readFileSync(import.meta.filename, "utf8");
  const start = source.lastIndexOf('const selectorProbes = identity === "-"');
  const end = source.indexOf('}).join("\\n");', start) + '}).join("\\n");'.length;
  expect(start).toBeGreaterThan(0);
  expect(end).toBeGreaterThan(start);
  const calls: string[][] = [];
  runInNewContext(source.slice(start, end), {
    identity: "fixture-identity", sha1: "fixture-sha1", keychain: "/fixture/private.keychain-db",
    identifier: "fixture.host", requirement: "fixture-requirement", binary: "/fixture/synthetic-host",
    file: (name: string) => `/fixture/${name}`,
    spawnSync: (_command: string, args: string[]) => { calls.push(args); return { status: 0, stderr: "" }; },
  });
  expect(calls).toHaveLength(6);
  for (const args of calls) expect(args.at(-1)).toBe("/fixture/synthetic-host");
});

test("remote key compatibility never loads native code for private windows or Windows", () => {
  let calls = 0;
  const load = () => { calls++; throw new Error("must not load"); };
  expect(installRemoteKeyCompatibility({ platform: "win32", incognito: false, load })).toBe(false);
  expect(installRemoteKeyCompatibility({ platform: "darwin", incognito: true, load })).toBe(false);
  expect(calls).toBe(0);
});

test("native compatibility failure never prevents official startup", () => {
  expect(installRemoteKeyCompatibility({ platform: "darwin", incognito: false, directory: "/missing" })).toBe(false);
});

test.skipIf(process.platform !== "darwin")("device-key adapter is independently source-pinned and not a Runtime catalog addon", () => {
  const root = join(import.meta.dir, "../../native/macos");
  const addonName = "incodex-device-key-adapter.node";
  const addon = readFileSync(join(root, "dist", addonName));
  const manifestBytes = readFileSync(join(root, "dist/incodex-device-key-adapter-manifest.json"));
  const manifest = JSON.parse(manifestBytes.toString("utf8"));
  const sourceBytes = Buffer.concat(
    deviceKeyAdapterSources.map(source => readFileSync(join(root, source))),
  );
  const digest = (bytes: Buffer) => createHash("sha256").update(bytes).digest("hex");

  expect(manifestBytes.at(-1)).toBe(0x0a);
  expect(Object.keys(manifest).sort()).toEqual([
    "abiVersion", "architectures", "files", "minimumMacOS", "platform", "schemaVersion", "sourceSha256",
  ]);
  expect(manifest).toMatchObject({
    schemaVersion: 1,
    platform: "macos",
    abiVersion: 1,
    minimumMacOS: "12.0",
    architectures: ["arm64", "x86_64"],
    sourceSha256: digest(sourceBytes),
    files: { [addonName]: digest(addon) },
  });
  expect(Object.keys(manifest.files)).toEqual([addonName]);
  expect(addon.length).toBeGreaterThan(4);
  expect([0xcafebabe, 0xcffaedfe, 0xfeedface, 0xfeedfacf]).toContain(addon.readUInt32BE(0));
});

test.skipIf(process.platform !== "darwin")("new adapter preflight validates host and official module without fishhook state", () => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-device-adapter-host-"));
  try {
    const root = join(import.meta.dir, "../../native/macos");
    const binary = join(directory, "adapter-host-test");
    const compiled = spawnSync("xcrun", ["clang", "-fobjc-arc", "-Wno-deprecated-declarations", "-framework", "Foundation",
      "-framework", "Security", join(root, "device-key-adapter-host.test.m"), "-o", binary], { encoding: "utf8" });
    expect(compiled.status).toBe(0);
    const result = spawnSync(binary, [], { encoding: "utf8" });
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("6 checks, 0 failures");
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test.skipIf(process.platform !== "darwin")("native host gate accepts only ad-hoc or the registered local certificate", () => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-remote-host-test-"));
  try {
    const binary = join(directory, "host-test");
    const root = join(import.meta.dir, "../../native/macos");
    const compiled = spawnSync("xcrun", ["clang", "-fobjc-arc", "-Wno-deprecated-declarations", "-undefined", "dynamic_lookup", "-framework", "Foundation", "-framework", "Security", join(root, "remote-key-host.test.m"), join(root, "vendor/fishhook/fishhook.c"), "-o", binary], { encoding: "utf8" });
    expect(compiled.stderr).not.toContain("error:");
    expect(compiled.status).toBe(0);
    const result = spawnSync(binary, [], { encoding: "utf8" });
    expect(result.stderr).toBe("");
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("33 checks, 0 failures");
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test.skipIf(process.platform !== "darwin" || process.env.INCODEX_LOCAL_SIGNING_ACCEPTANCE !== "1")("host identity verifies real local signatures without private assets or search-list changes", () => {
  const run = (command: string, args: string[]) => {
    const result = spawnSync(command, args, { encoding: "utf8", timeout: 30_000 });
    if (result.error || result.status !== 0) throw result.error ?? new Error(`${command} failed: ${result.stderr}`);
    return result.stdout;
  };
  const searchList = () => run("/usr/bin/security", ["list-keychains", "-d", "user"]);
  const before = searchList();
  const directory = realpathSync(mkdtempSync(join(tmpdir(), "incodex-identity-real-")));
  try {
    chmodSync(directory, 0o700);
    const root = join(import.meta.dir, "../../native/macos");
    const store = join(directory, "test-signing-store");
    const binary = join(directory, "test-host");
    const home = join(directory, "home");
    const state = join(home, ".incodex/macos-signing");
    mkdirSync(state, { recursive: true, mode: 0o700 });
    const file = (name: string) => join(directory, name);
    const privateFile = (path: string, bytes: string | Buffer) => writeFileSync(path, bytes, { mode: 0o600 });
    privateFile(file("password"), randomBytes(32).toString("hex"));
    privateFile(file("certificate.cnf"), "[req]\nprompt=no\ndistinguished_name=dn\nx509_extensions=code_signing\n[dn]\nCN=Incodex Isolated Identity Test\n[code_signing]\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=critical,codeSigning\n");
    run("xcrun", ["clang", "-fobjc-arc", "-Wno-deprecated-declarations", "-framework", "Foundation", "-framework", "Security", join(import.meta.dir, "../../labs/macos-ax-continuity/identity-store.m"), "-o", store]);
    run("xcrun", ["clang", "-fobjc-arc", "-Wno-deprecated-declarations", "-framework", "Foundation", "-framework", "Security", join(root, "host-identity-real.test.m"), "-o", binary]);
    run("/usr/bin/openssl", ["req", "-new", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2", "-config", file("certificate.cnf"), "-keyout", file("key.pem"), "-out", file("certificate.pem")]);
    run("/usr/bin/openssl", ["x509", "-in", file("certificate.pem"), "-outform", "DER", "-out", file("certificate.der")]);
    run("/usr/bin/openssl", ["pkcs12", "-export", "-inkey", file("key.pem"), "-in", file("certificate.pem"), "-out", file("identity.p12"), "-passout", `file:${file("password")}`]);
    for (const name of ["key.pem", "certificate.pem", "certificate.der", "identity.p12"]) chmodSync(file(name), 0o600);
    // --- 专属测试钥匙串：复用不改 search list 的实验后端。 ---
    run(store, ["create", file("identity.keychain-db"), file("password"), file("identity.p12")]);
    const certificate = readFileSync(file("certificate.der"));
    const hash = (algorithm: string, bytes: Buffer) => createHash(algorithm).update(bytes).digest("hex");
    const sha1 = hash("sha1", certificate);
    const storeHash = hash("sha256", readFileSync(store));
    const registration = {
      schemaVersion: 1, keychainPath: join(state, "identity.keychain-db"),
      certificateSha1: sha1, certificateSha256: hash("sha256", certificate),
      storePath: join(home, ".incodex/helpers/macos-signing", storeHash, "incodex-signing-store"), storeSha256: storeHash,
    };
    privateFile(join(state, "certificate.der"), certificate);
    privateFile(join(state, "identity.json"), JSON.stringify(registration));
    const sign = (identity: string, identifier: string) => {
      try {
        return run("/usr/bin/codesign", ["--force", "--sign", identity,
          "--keychain", file("identity.keychain-db"), "--identifier", identifier, "--options", "runtime", "--timestamp=none",
          "--requirements", `=designated => identifier "${identifier}"${identity === "-" ? "" : ` and certificate leaf = H"${sha1}"`}`, binary]);
      } catch (error) {
        // --- 只查询本测试的临时身份；不改信任或全局搜索列表。 ---
        const probes = [
          ["/usr/bin/security", ["find-identity", "-p", "codesigning", file("identity.keychain-db")]],
          ["/usr/bin/openssl", ["x509", "-in", file("certificate.pem"), "-noout", "-dates", "-subject", "-text"]],
        ] as const;
        const diagnostics = probes.map(([command, args]) => {
          const result = spawnSync(command, [...args], { encoding: "utf8", timeout: 5_000 });
          const output = command.endsWith("openssl")
            ? (result.stdout.match(/(?:notBefore=|notAfter=|subject=|Signature Algorithm:)[^\r\n]*/g) ?? []).join("\n")
            : result.stdout + result.stderr;
          return `${command}: exit=${result.status} ${output.slice(0, 4096)}`;
        }).join("\n");
        // 原始签名已失败才查询，避免诊断预热改变待测路径。
        const query = spawnSync(binary, ["--identity-query", file("identity.keychain-db"), sha1], { encoding: "utf8", timeout: 5_000 });
        const identityQuery = `exit=${query.status} ${(query.stdout + query.stderr).slice(0, 4096)}`;
        const imports = spawnSync("/usr/bin/xcrun", ["nm", "-u", "/usr/bin/codesign"], { encoding: "utf8", timeout: 5_000 });
        const identityImports = `nm-exit=${imports.status} modern-query=${imports.stdout?.includes("_SecItemCopyMatching")} legacy-query=${imports.stdout?.includes("_SecIdentitySearchCreateWithPolicy")}`;
        const keychain = file("identity.keychain-db");
        const requirement = `=designated => identifier "${identifier}" and certificate leaf = H"${sha1}"`;
        const selectorProbes = identity === "-" ? [] : [
          ["same-arguments", sha1, keychain, true, true],
          ["without-runtime", sha1, keychain, false, true],
          ["without-requirement", sha1, keychain, true, false],
          ["uppercase-exact", sha1.toUpperCase(), keychain, true, true],
          ["common-name-exact", "Incodex Isolated Identity Test", keychain, true, true],
          ["logical-keychain-path", sha1, file("identity.keychain"), true, true],
        ].map(([label, selector, keychainPath, runtime, withRequirement]) => {
          // --- 仅复放临时目标；单变量对照，成功也不抹去原失败。 ---
          const args = ["--force", "--sign", String(selector), "--keychain", String(keychainPath),
            "--identifier", identifier, "--timestamp=none"];
          if (runtime) args.push("--options", "runtime");
          if (withRequirement) args.push("--requirements", requirement);
          args.push(binary);
          const result = spawnSync("/usr/bin/codesign", args, { encoding: "utf8", timeout: 5_000 });
          return `${label}: exit=${result.status} ${(result.stderr ?? "").slice(0, 512)}`;
        }).join("\n");
        throw new Error(`${String(error)}\nTemporary signing fixture diagnostics:\n${diagnostics}\nRead-only identity queries:\n${identityQuery}\nCodesign query imports:\n${identityImports}\nSelector probes:\n${selectorProbes}`);
      }
    };
    sign(sha1, "com.openai.codex");
    run("/usr/bin/codesign", ["--verify", "--strict", binary]);
    expect(run(binary, [home]).trim()).toBe("registered-local");
    privateFile(join(state, "identity.json"), JSON.stringify({ ...registration, certificateSha256: "invalid" }));
    expect(run(binary, [home]).trim()).toBe("unsupported");
    privateFile(join(state, "identity.json"), JSON.stringify(registration));
    sign(sha1, "unrelated.host");
    expect(run(binary, [home]).trim()).toBe("unsupported");
    sign("-", "com.openai.codex");
    rmSync(state, { recursive: true });
    expect(run(binary, [home]).trim()).toBe("ad-hoc");
  } finally {
    try { expect(searchList() === before).toBe(true); }
    finally { rmSync(directory, { recursive: true, force: true }); }
  }
}, 120_000);

// This owns a disposable keychain; it never uses the user's login keychain.
test.skipIf(process.platform !== "darwin")("native policy preserves signing and blocks private-key export", () => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-remote-policy-"));
  try {
    const binary = join(directory, "policy-test");
    const source = join(import.meta.dir, "../../native/macos/remote-key-compat.test.m");
    const compiled = spawnSync("xcrun", ["clang", "-fobjc-arc", "-Wno-deprecated-declarations", "-framework", "Foundation", "-framework", "Security", source, "-o", binary]);
    expect(compiled.status).toBe(0);
    const result = spawnSync(binary, [], { encoding: "utf8" });
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("real keychain contracts passed");
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test.skipIf(process.platform !== "darwin")("successful native retry clears the released original error", () => {
  const directory = mkdtempSync(join(tmpdir(), "incodex-remote-error-"));
  try {
    const binary = join(directory, "error-test");
    const root = join(import.meta.dir, "../../native/macos");
    const compiled = spawnSync("xcrun", ["clang", "-fobjc-arc", "-Wno-deprecated-declarations", "-undefined", "dynamic_lookup", "-framework", "Foundation", "-framework", "Security", join(root, "remote-key-error.test.m"), join(root, "vendor/fishhook/fishhook.c"), "-o", binary]);
    expect(compiled.status).toBe(0);
    const result = spawnSync(binary, [], { encoding: "utf8" });
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("retry error ownership passed");
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test.skipIf(process.platform !== "darwin")("native device bridge validates fixed identity and bounded child transport", () => {
  const directory = realpathSync(mkdtempSync(join(tmpdir(), "incodex-device-transport-")));
  chmodSync(directory, 0o700);
  try {
    const binary = join(directory, "transport-test");
    const root = join(import.meta.dir, "../../native/macos");
    const compiled = spawnSync("xcrun", ["clang", "-fobjc-arc", "-Wno-deprecated-declarations", "-framework", "Foundation",
      "-framework", "Security", join(root, "device-key-bridge.test.m"), "-o", binary], { encoding: "utf8" });
    expect(compiled.stderr).not.toContain("error:");
    expect(compiled.status).toBe(0);
    const result = spawnSync(binary, [directory], { encoding: "utf8", timeout: 10000 });
    expect(result.stderr).toBe("");
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("device helper transport contracts passed");
  } finally { rmSync(directory, { recursive: true, force: true }); }
}, 20_000);


test.skipIf(process.platform !== "darwin")("device consumer preserves existing P256 identity and closed signing domain", () => {
  const directory = realpathSync(mkdtempSync(join(tmpdir(), "incodex-device-consumer-suite-")));
  chmodSync(directory, 0o700);
  try {
    const binary = join(directory, "consumer-test");
    const root = join(import.meta.dir, "../../native/macos");
    const compiled = spawnSync("xcrun", ["clang", "-fobjc-arc", "-Wno-deprecated-declarations", "-DINCODEX_DEVICE_KEY_CONSUMER_TESTING",
      "-framework", "Foundation", "-framework", "Security", join(root, "device-key-consumer.test.m"), join(root, "device-key-helper.m"), "-o", binary], { encoding: "utf8" });
    expect(compiled.stderr).not.toContain("error:");
    expect(compiled.status).toBe(0);
    const result = spawnSync(binary, [], { encoding: "utf8", timeout: 30000 });
    expect(result.stderr).toBe("");
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("device-key consumer isolated Keychain contracts passed");
  } finally { rmSync(directory, { recursive: true, force: true }); }
}, 60_000);


test.skipIf(process.platform !== "darwin")("device helper retires when its exact parent exits", () => {
  const directory = realpathSync(mkdtempSync(join(tmpdir(), "incodex-device-lifetime-suite-")));
  chmodSync(directory, 0o700);
  try {
    const binary = join(directory, "lifecycle-test");
    const root = join(import.meta.dir, "../../native/macos");
    const compiled = spawnSync("xcrun", ["clang", "-fobjc-arc", "-Wno-deprecated-declarations", "-DINCODEX_DEVICE_KEY_LIFECYCLE_TESTING",
      "-framework", "Foundation", "-framework", "Security", join(root, "device-key-helper-lifecycle.test.m"), join(root, "device-key-helper.m"), "-o", binary], { encoding: "utf8" });
    expect(compiled.stderr).not.toContain("error:");
    expect(compiled.status).toBe(0);
    const result = spawnSync(binary, [], { encoding: "utf8", timeout: 10000 });
    expect(result.stderr).toBe("");
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("device-key helper parent-exit lifecycle passed");
  } finally { rmSync(directory, { recursive: true, force: true }); }
}, 20_000);


test.skipIf(process.platform !== "darwin")("N-API workers preserve Buffer requests and official denial codes", async () => {
  const directory = realpathSync(mkdtempSync(join(tmpdir(), "incodex-device-napi-")));
  chmodSync(directory, 0o700);
  try {
    const addon = buildDeviceNapiFixture(directory);
    const native = require(addon);
    expect(native.prepareDeviceKeyAccess()).toBe(true);
    const key = `dk_osn_${"A".repeat(43)}`;
    const pending = native.getDeviceKeyPublic(key);
    expect(pending).toBeInstanceOf(Promise);
    expect(await pending).toMatchObject({ keyId: key, algorithm: "ecdsa_p256_sha256", publicKeySpkiDerBase64: "synthetic-public-result" });
    let denied: any;
    try { await native.signDeviceKey(key, Buffer.from("synthetic-buffer-challenge")); } catch (error) { denied = error; }
    expect(denied).toBeInstanceOf(Error);
    expect(denied.errorCode).toBe("remote_control_device_key_store_unavailable");
    expect(denied.osStatus).toBe(-25293);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}, 60_000);


test.skipIf(process.platform !== "darwin")("pending N-API authorization wait does not occupy Node's sole worker", async () => {
  const directory = realpathSync(mkdtempSync(join(tmpdir(), "incodex-device-napi-pending-")));
  chmodSync(directory, 0o700);
  const ready = join(directory, "helper-ready");
  const gateRelease = join(directory, "release-helper");
  const statDone = join(directory, "node-fs-stat-done");
  const signDone = join(directory, "sign-result");
  const poolSize = join(directory, "node-threadpool-size");
  let nodeChild: ReturnType<typeof spawn> | undefined;
  let nodeExit: Promise<{ code: number | null; signal: NodeJS.Signals | null }> | undefined;
  let exitResult: { code: number | null; signal: NodeJS.Signals | null } | undefined;
  let signResult: string | undefined;
  let childStderr = "";
  try {
    const addon = buildDeviceNapiFixture(directory);
    const nodeProgram = `
      const fs = require("node:fs");
      const native = require(process.env.INCODEX_DEVICE_TEST_ADDON);
      const key = "dk_osn_" + "A".repeat(43);
      const keepAlive = setInterval(() => {}, 1000);
      async function main() {
        if (!native.prepareDeviceKeyAccess()) throw new Error("synthetic helper was not prepared");
        const publicKey = await native.getDeviceKeyPublic(key);
        if (publicKey.publicKeySpkiDerBase64 !== "synthetic-public-result") throw new Error("synthetic public response changed");
        fs.writeFileSync(process.env.INCODEX_DEVICE_TEST_POOL_SIZE, process.env.UV_THREADPOOL_SIZE || "unset", { mode: 0o600 });
        const sign = native.signDeviceKey(key, Buffer.from("synthetic-buffer-challenge")).then(
          () => { throw new Error("synthetic sign fixture unexpectedly succeeded"); },
          (error) => {
            if (error.errorCode !== "remote_control_device_key_store_unavailable" || error.osStatus !== -25293) {
              throw new Error("synthetic denial code changed");
            }
            fs.writeFileSync(process.env.INCODEX_DEVICE_TEST_SIGN_DONE, "denied", { mode: 0o600 });
          },
        );
        const gatePoll = setInterval(() => {
          if (!fs.existsSync(process.env.INCODEX_DEVICE_TEST_GATE_READY)) return;
          clearInterval(gatePoll);
          fs.stat(process.execPath, (error) => {
            if (error) { process.stderr.write("node fs.stat failed"); process.exitCode = 1; return; }
            fs.writeFileSync(process.env.INCODEX_DEVICE_TEST_STAT_DONE, "done", { mode: 0o600 });
          });
        }, 5);
        await sign;
        clearInterval(keepAlive);
      }
      main().catch((error) => { clearInterval(keepAlive); process.stderr.write(error.message); process.exitCode = 1; });
    `;
    nodeChild = spawn("node", ["-e", nodeProgram], {
      env: {
        ...process.env,
        UV_THREADPOOL_SIZE: "1",
        INCODEX_DEVICE_TEST_ADDON: addon,
        INCODEX_DEVICE_TEST_SIGN_DONE: signDone,
        INCODEX_DEVICE_TEST_STAT_DONE: statDone,
        INCODEX_DEVICE_TEST_POOL_SIZE: poolSize,
        INCODEX_DEVICE_TEST_GATE_OPERATION: "sign",
        INCODEX_DEVICE_TEST_GATE_READY: ready,
        INCODEX_DEVICE_TEST_GATE_RELEASE: gateRelease,
      },
      stdio: ["ignore", "ignore", "pipe"],
    });
    if (nodeChild.stderr) {
      nodeChild.stderr.setEncoding("utf8");
      nodeChild.stderr.on("data", (chunk: string) => { childStderr += chunk; });
    }
    nodeExit = new Promise((resolve, reject) => {
      nodeChild?.once("error", reject);
      nodeChild?.once("exit", (code, signal) => resolve({ code, signal }));
    });
    const waitForFile = async (path: string, timeoutMs: number) => {
      const until = Date.now() + timeoutMs;
      while (Date.now() < until) {
        if (existsSync(path)) return true;
        await Bun.sleep(10);
      }
      return existsSync(path);
    };
    expect(await waitForFile(ready, 10_000)).toBe(true);
    const readyMatch = /^stdin-eof:(\d+)$/.exec(readFileSync(ready, "utf8"));
    expect(readyMatch).not.toBeNull();
    expect(readFileSync(poolSize, "utf8")).toBe("1");
    const helperPid = Number(readyMatch?.[1]);
    expect(existsSync(gateRelease)).toBe(false);
    expect(existsSync(signDone)).toBe(false);
    expect(() => process.kill(helperPid, 0)).not.toThrow();
    console.log("pending-sign=true helper-alive=true release=false sign-done=false uv-threadpool-size=1");
    expect(await waitForFile(statDone, 500)).toBe(true);
    expect(existsSync(gateRelease)).toBe(false);
    expect(existsSync(signDone)).toBe(false);
    expect(() => process.kill(helperPid, 0)).not.toThrow();
  } finally {
    writeFileSync(gateRelease, "release", { mode: 0o600 });
    if (nodeChild && nodeExit) {
      exitResult = await Promise.race([nodeExit.catch(() => null), Bun.sleep(10_000).then(() => null)]) ?? undefined;
      if (exitResult === undefined) {
        nodeChild.kill("SIGKILL");
        exitResult = await Promise.race([nodeExit.catch(() => null), Bun.sleep(1_000).then(() => null)]) ?? undefined;
      }
    }
    if (existsSync(signDone)) signResult = readFileSync(signDone, "utf8");
    rmSync(directory, { recursive: true, force: true });
  }
  expect(exitResult?.code).toBe(0);
  expect(childStderr).toBe("");
  expect(signResult).toBe("denied");
}, 60_000);


test.skipIf(process.platform !== "darwin")("sign approval remains valid after its machine deadline", async () => {
  const directory = realpathSync(mkdtempSync(join(tmpdir(), "incodex-device-napi-late-approval-")));
  chmodSync(directory, 0o700);
  const publicReady = join(directory, "public-helper-ready");
  const publicRelease = join(directory, "public-release-helper");
  const publicDone = join(directory, "public-result");
  const signReady = join(directory, "sign-helper-ready");
  const signRelease = join(directory, "sign-release-helper");
  const signDone = join(directory, "sign-result");
  let nodeChild: ReturnType<typeof spawn> | undefined;
  let nodeExit: Promise<{ code: number | null; signal: NodeJS.Signals | null }> | undefined;
  let exitResult: { code: number | null; signal: NodeJS.Signals | null } | undefined;
  let signResult: string | undefined;
  let childStderr = "";
  let settledBeforeApproval = false;
  try {
    const addon = buildDeviceNapiFixture(directory, ["-DINCODEX_DEVICE_HELPER_DEADLINE_SECONDS=0.3"]);
    const nodeProgram = `
      const fs = require("node:fs");
      const native = require(process.env.INCODEX_DEVICE_TEST_ADDON);
      const key = "dk_osn_" + "A".repeat(43);
      const keepAlive = setInterval(() => {}, 1000);
      async function waitForFile(path) {
        const until = Date.now() + 10000;
        while (!fs.existsSync(path) && Date.now() < until) await new Promise((resolve) => setTimeout(resolve, 5));
        if (!fs.existsSync(path)) throw new Error("synthetic helper did not reach its approval gate");
      }
      async function main() {
        if (!native.prepareDeviceKeyAccess()) throw new Error("synthetic helper was not prepared");
        const publicResult = native.getDeviceKeyPublic(key).then(
          () => "resolved",
          (error) => error.errorCode === "remote_control_device_key_store_unavailable" ? "rejected" : "unexpected",
        );
        await waitForFile(process.env.INCODEX_DEVICE_TEST_GATE_READY);
        await publicResult.then((value) => fs.writeFileSync(process.env.INCODEX_DEVICE_TEST_PUBLIC_DONE, value, { mode: 0o600 }));
        process.env.INCODEX_DEVICE_TEST_GATE_OPERATION = "sign";
        process.env.INCODEX_DEVICE_TEST_GATE_READY = process.env.INCODEX_DEVICE_TEST_SIGN_READY;
        process.env.INCODEX_DEVICE_TEST_GATE_RELEASE = process.env.INCODEX_DEVICE_TEST_SIGN_RELEASE;
        const result = await native.signDeviceKey(key, Buffer.from("synthetic-buffer-challenge")).then(
          () => "signed",
          (error) => error.errorCode === "remote_control_device_key_store_unavailable" ? "rejected" : "unexpected",
        );
        fs.writeFileSync(process.env.INCODEX_DEVICE_TEST_SIGN_DONE, result, { mode: 0o600 });
        clearInterval(keepAlive);
      }
      main().catch((error) => { clearInterval(keepAlive); process.stderr.write(error.message); process.exitCode = 1; });
    `;
    nodeChild = spawn("node", ["-e", nodeProgram], {
      env: {
        ...process.env,
        INCODEX_DEVICE_TEST_ADDON: addon,
        INCODEX_DEVICE_TEST_SIGN_DONE: signDone,
        INCODEX_DEVICE_TEST_PUBLIC_DONE: publicDone,
        INCODEX_DEVICE_TEST_GATE_OPERATION: "public",
        INCODEX_DEVICE_TEST_GATE_READY: publicReady,
        INCODEX_DEVICE_TEST_GATE_RELEASE: publicRelease,
        INCODEX_DEVICE_TEST_SIGN_READY: signReady,
        INCODEX_DEVICE_TEST_SIGN_RELEASE: signRelease,
        INCODEX_DEVICE_TEST_SIGN_RESULT: "success",
      },
      stdio: ["ignore", "ignore", "pipe"],
    });
    if (nodeChild.stderr) {
      nodeChild.stderr.setEncoding("utf8");
      nodeChild.stderr.on("data", (chunk: string) => { childStderr += chunk; });
    }
    nodeExit = new Promise((resolve, reject) => {
      nodeChild?.once("error", reject);
      nodeChild?.once("exit", (code, signal) => resolve({ code, signal }));
    });
    const waitForFile = async (path: string, timeoutMs: number) => {
      const until = Date.now() + timeoutMs;
      while (Date.now() < until) {
        if (existsSync(path)) return true;
        await Bun.sleep(10);
      }
      return existsSync(path);
    };
    expect(await waitForFile(publicDone, 5_000)).toBe(true);
    expect(readFileSync(publicDone, "utf8")).toBe("rejected");
    expect(/^stdin-eof:\d+$/.test(readFileSync(publicReady, "utf8"))).toBe(true);
    expect(await waitForFile(signReady, 10_000)).toBe(true);
    expect(/^stdin-eof:\d+$/.test(readFileSync(signReady, "utf8"))).toBe(true);
    expect(existsSync(publicRelease)).toBe(false);
    expect(existsSync(signRelease)).toBe(false);
    // 夹具仅注入0.3秒截止；正式构建仍使用300秒。
    await Bun.sleep(600);
    settledBeforeApproval = existsSync(signDone);
    console.log(`public-deadline-bounded=true approval-delay-past-machine-deadline=true request-eof=true sign-settled-before-release=${settledBeforeApproval}`);
    writeFileSync(signRelease, "release", { mode: 0o600 });
    exitResult = await Promise.race([nodeExit.catch(() => null), Bun.sleep(10_000).then(() => null)]) ?? undefined;
    if (exitResult === undefined) {
      nodeChild.kill("SIGKILL");
      exitResult = await Promise.race([nodeExit.catch(() => null), Bun.sleep(1_000).then(() => null)]) ?? undefined;
    }
  } finally {
    writeFileSync(publicRelease, "release", { mode: 0o600 });
    writeFileSync(signRelease, "release", { mode: 0o600 });
    if (nodeChild && nodeExit && exitResult === undefined) {
      exitResult = await Promise.race([nodeExit.catch(() => null), Bun.sleep(10_000).then(() => null)]) ?? undefined;
      if (exitResult === undefined) {
        nodeChild.kill("SIGKILL");
        exitResult = await Promise.race([nodeExit.catch(() => null), Bun.sleep(1_000).then(() => null)]) ?? undefined;
      }
    }
    if (existsSync(signDone)) signResult = readFileSync(signDone, "utf8");
    rmSync(directory, { recursive: true, force: true });
  }
  expect(exitResult?.code).toBe(0);
  expect(childStderr).toBe("");
  expect({ settledBeforeApproval, signResult }).toEqual({ settledBeforeApproval: false, signResult: "signed" });
}, 60_000);


test.skipIf(process.platform !== "darwin")("Worker teardown cancels its own pending device helper", async () => {
  const directory = realpathSync(mkdtempSync(join(tmpdir(), "incodex-device-napi-worker-teardown-")));
  chmodSync(directory, 0o700);
  const gateRelease = join(directory, "release-helper");
  const progress = join(directory, "worker-progress.jsonl");
  const report = join(directory, "worker-teardown-result");
  let nodeChild: ReturnType<typeof spawn> | undefined;
  let nodeExit: Promise<{ code: number | null; signal: NodeJS.Signals | null }> | undefined;
  let exitResult: { code: number | null; signal: NodeJS.Signals | null } | undefined;
  let result: Array<{ terminateCompleted: boolean; helperAliveAfterTerminate: boolean; releasedBeforeTerminate: boolean }> | undefined;
  let childStderr = "";
  let reportReady = false;
  let lastProgress = "none";
  try {
    const addon = buildDeviceNapiFixture(directory);
    const workerSource = `
      const { workerData } = require("node:worker_threads");
      const native = require(workerData.addon);
      const keepAlive = setInterval(() => {}, 1000);
      if (!native.prepareDeviceKeyAccess()) throw new Error("synthetic helper was not prepared");
      native.signDeviceKey("dk_osn_" + "A".repeat(43), Buffer.from("synthetic-buffer-challenge"))
        .then(() => clearInterval(keepAlive), () => clearInterval(keepAlive));
    `;
    const nodeProgram = `
      const fs = require("node:fs");
      const { Worker } = require("node:worker_threads");
      const workerSource = ${JSON.stringify(workerSource)};
      const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
      const trace = (phase, index) => fs.appendFileSync(process.env.INCODEX_DEVICE_TEST_PROGRESS, JSON.stringify({ phase, index }) + "\\n", { mode: 0o600 });
      async function waitForFile(path, timeoutMs) {
        const until = Date.now() + timeoutMs;
        while (!fs.existsSync(path) && Date.now() < until) await delay(5);
        return fs.existsSync(path);
      }
      async function main() {
        const cases = [];
        const workers = [];
        try {
          for (let index = 0; index < 8; index++) {
            trace("case-start", index);
            const ready = process.env.INCODEX_DEVICE_TEST_GATE_READY;
            const release = process.env.INCODEX_DEVICE_TEST_GATE_RELEASE;
            fs.rmSync(ready, { force: true });
            fs.rmSync(release, { force: true });
            const worker = new Worker(workerSource, { eval: true, workerData: { addon: process.env.INCODEX_DEVICE_TEST_ADDON } });
            workers.push(worker);
            trace("worker-created", index);
            if (!await waitForFile(ready, 10000)) { trace("helper-ready-timeout", index); throw new Error("synthetic helper did not reach its approval gate"); }
            trace("helper-ready", index);
            const marker = fs.readFileSync(ready, "utf8");
            const match = /^stdin-eof:(\\d+)$/.exec(marker);
            if (!match) { trace("helper-marker-invalid", index); throw new Error("synthetic EOF marker is invalid"); }
            const helperPid = Number(match[1]);
            const releasedBeforeTerminate = index % 2 === 1;
            if (releasedBeforeTerminate) { fs.writeFileSync(release, "release", { mode: 0o600 }); trace("released-before-terminate", index); }
            let terminateCompleted = false;
            try {
              terminateCompleted = await Promise.race([
                worker.terminate().then(() => true),
                delay(500).then(() => false),
              ]);
            } catch {}
            trace(terminateCompleted ? "terminate-complete" : "terminate-timeout", index);
            let helperAliveAfterTerminate = false;
            try { process.kill(helperPid, 0); helperAliveAfterTerminate = true; } catch {}
            trace(helperAliveAfterTerminate ? "helper-alive-after-terminate" : "helper-reaped", index);
            if (!terminateCompleted) await worker.terminate().catch(() => {});
            cases.push({ terminateCompleted, helperAliveAfterTerminate, releasedBeforeTerminate });
            trace("case-complete", index);
          }
          fs.writeFileSync(process.env.INCODEX_DEVICE_TEST_REPORT, JSON.stringify(cases), { mode: 0o600 });
          trace("report-written", 8);
        } finally {
          trace("cleanup-release", -1);
          fs.writeFileSync(process.env.INCODEX_DEVICE_TEST_GATE_RELEASE, "release", { mode: 0o600 });
          trace("cleanup-terminate-start", -1);
          await Promise.all(workers.map((worker) => worker.terminate().catch(() => {})));
          trace("cleanup-complete", -1);
        }
      }
      main().catch((error) => { process.stderr.write(error.message); process.exitCode = 1; });
    `;
    nodeChild = spawn("node", ["-e", nodeProgram], {
      env: {
        ...process.env,
        INCODEX_DEVICE_TEST_ADDON: addon,
        INCODEX_DEVICE_TEST_GATE_OPERATION: "sign",
        INCODEX_DEVICE_TEST_GATE_READY: join(directory, "helper-ready"),
        INCODEX_DEVICE_TEST_GATE_RELEASE: gateRelease,
        INCODEX_DEVICE_TEST_PROGRESS: progress,
        INCODEX_DEVICE_TEST_REPORT: report,
        INCODEX_DEVICE_TEST_SIGN_RESULT: "success",
      },
      stdio: ["ignore", "ignore", "pipe"],
    });
    if (nodeChild.stderr) {
      nodeChild.stderr.setEncoding("utf8");
      nodeChild.stderr.on("data", (chunk: string) => { childStderr += chunk; });
    }
    nodeExit = new Promise((resolve, reject) => {
      nodeChild?.once("error", reject);
      nodeChild?.once("exit", (code, signal) => resolve({ code, signal }));
    });
    const waitForFile = async (path: string, timeoutMs: number) => {
      const until = Date.now() + timeoutMs;
      while (Date.now() < until) {
        if (existsSync(path)) return true;
        await Bun.sleep(10);
      }
      return existsSync(path);
    };
    reportReady = await waitForFile(report, 10_000);
    if (reportReady) result = JSON.parse(readFileSync(report, "utf8"));
  } finally {
    writeFileSync(gateRelease, "release", { mode: 0o600 });
    if (nodeChild && nodeExit) {
      exitResult = await Promise.race([nodeExit.catch(() => null), Bun.sleep(10_000).then(() => null)]) ?? undefined;
      if (exitResult === undefined) {
        nodeChild.kill("SIGKILL");
        exitResult = await Promise.race([nodeExit.catch(() => null), Bun.sleep(1_000).then(() => null)]) ?? undefined;
      }
    }
    if (existsSync(progress)) lastProgress = readFileSync(progress, "utf8").trim().split("\n").at(-1) ?? "empty";
    rmSync(directory, { recursive: true, force: true });
  }
  if (!reportReady) throw new Error(`worker teardown incomplete ${JSON.stringify({ exitResult: exitResult ?? "not-exited", childStderr: childStderr.replaceAll(directory, "<fixture>").slice(0, 512), lastProgress })}`);
  expect(reportReady).toBe(true);
  expect(exitResult?.code).toBe(0);
  expect(childStderr).toBe("");
  expect(result).toHaveLength(8);
  expect(result?.every((item) => item.terminateCompleted && !item.helperAliveAfterTerminate)).toBe(true);
  expect(result?.filter((item) => item.releasedBeforeTerminate)).toHaveLength(4);
}, 60_000);


test.skipIf(process.platform !== "darwin")("idle Node exit cancels a pending device helper", async () => {
  const directory = realpathSync(mkdtempSync(join(tmpdir(), "incodex-device-napi-idle-exit-")));
  chmodSync(directory, 0o700);
  const ready = join(directory, "helper-ready");
  const gateRelease = join(directory, "release-helper");
  const signDone = join(directory, "sign-result");
  let nodeChild: ReturnType<typeof spawn> | undefined;
  let nodeExit: Promise<{ code: number | null; signal: NodeJS.Signals | null }> | undefined;
  let exitBeforeRelease = false;
  let helperAliveBeforeRelease = false;
  let childStderr = "";
  try {
    const addon = buildDeviceNapiFixture(directory);
    const nodeProgram = `
      const fs = require("node:fs");
      const native = require(process.env.INCODEX_DEVICE_TEST_ADDON);
      if (!native.prepareDeviceKeyAccess()) throw new Error("synthetic helper was not prepared");
      const until = Date.now() + 10000;
      const readinessPoll = setInterval(() => {
        if (fs.existsSync(process.env.INCODEX_DEVICE_TEST_GATE_READY)) {
          clearInterval(readinessPoll);
        } else if (Date.now() >= until) {
          clearInterval(readinessPoll);
          process.exitCode = 1;
        }
      }, 5);
      native.signDeviceKey("dk_osn_" + "A".repeat(43), Buffer.from("synthetic-buffer-challenge"))
        .then(() => fs.writeFileSync(process.env.INCODEX_DEVICE_TEST_SIGN_DONE, "resolved", { mode: 0o600 }),
          () => fs.writeFileSync(process.env.INCODEX_DEVICE_TEST_SIGN_DONE, "rejected", { mode: 0o600 }));
    `;
    nodeChild = spawn("node", ["-e", nodeProgram], {
      env: {
        ...process.env,
        INCODEX_DEVICE_TEST_ADDON: addon,
        INCODEX_DEVICE_TEST_GATE_OPERATION: "sign",
        INCODEX_DEVICE_TEST_GATE_READY: ready,
        INCODEX_DEVICE_TEST_GATE_RELEASE: gateRelease,
        INCODEX_DEVICE_TEST_SIGN_DONE: signDone,
        INCODEX_DEVICE_TEST_SIGN_RESULT: "success",
      },
      stdio: ["ignore", "ignore", "pipe"],
    });
    if (nodeChild.stderr) {
      nodeChild.stderr.setEncoding("utf8");
      nodeChild.stderr.on("data", (chunk: string) => { childStderr += chunk; });
    }
    nodeExit = new Promise((resolve, reject) => {
      nodeChild?.once("error", reject);
      nodeChild?.once("exit", (code, signal) => resolve({ code, signal }));
    });
    const waitForFile = async (path: string, timeoutMs: number) => {
      const until = Date.now() + timeoutMs;
      while (Date.now() < until) {
        if (existsSync(path)) return true;
        await Bun.sleep(10);
      }
      return existsSync(path);
    };
    expect(await waitForFile(ready, 10_000)).toBe(true);
    const match = /^stdin-eof:(\d+)$/.exec(readFileSync(ready, "utf8"));
    expect(match).not.toBeNull();
    const helperPid = Number(match?.[1]);
    expect(existsSync(gateRelease)).toBe(false);
    expect(existsSync(signDone)).toBe(false);
    expect(() => process.kill(helperPid, 0)).not.toThrow();
    const observedExit = await Promise.race([nodeExit.then(() => true), Bun.sleep(500).then(() => false)]);
    exitBeforeRelease = observedExit;
    try { process.kill(helperPid, 0); helperAliveBeforeRelease = true; } catch {}
  } finally {
    writeFileSync(gateRelease, "release", { mode: 0o600 });
    if (nodeChild && nodeExit) {
      const result = await Promise.race([nodeExit.catch(() => null), Bun.sleep(10_000).then(() => null)]);
      if (result === null) {
        nodeChild.kill("SIGKILL");
        await Promise.race([nodeExit.catch(() => null), Bun.sleep(1_000).then(() => null)]);
      }
    }
    rmSync(directory, { recursive: true, force: true });
  }
  expect({ exitBeforeRelease, helperAliveBeforeRelease, childStderr }).toEqual({
    exitBeforeRelease: true, helperAliveBeforeRelease: false, childStderr: "",
  });
}, 60_000);
