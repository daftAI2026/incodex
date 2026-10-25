/**
 * [INPUT]: 依赖 adapter 构建清单、合成宿主和临时 native fixture。
 * [OUTPUT]: 验证旧创建回退、独立 adapter 资产和异步 helper bridge。
 * [POS]: 密钥兼容与资产回归；不读用户 Keychain 或宿主密钥。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { spawnSync } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "bun:test";
import { runInNewContext } from "node:vm";
import { deviceKeyAdapterSources } from "../../scripts/build-device-key-adapter.ts";
import { installRemoteKeyCompatibility } from "./incodex-remote-key-compat.cts";

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
    expect(result.stdout).toContain("36 checks, 0 failures");
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
    const source = join(import.meta.dir, "../../native/macos/device-key-bridge-napi.test.m");
    const addon = join(directory, "fixture.node"), helper = join(directory, "fixture-helper");
    for (const args of [
      ["-shared", "-undefined", "dynamic_lookup", source, "-o", addon],
      ["-DINCODEX_DEVICE_BRIDGE_HELPER_FIXTURE", source, "-o", helper],
    ]) {
      const result = spawnSync("xcrun", ["clang", "-fobjc-arc", "-Wno-deprecated-declarations", "-framework", "Foundation", "-framework", "Security", `-DINCODEX_DEVICE_NAPI_FIXTURE_HOME="${directory}"`, ...args], { encoding: "utf8" });
      expect(result.stderr).not.toContain("error:"); expect(result.status).toBe(0);
    }
    const bytes = readFileSync(helper), digest = createHash("sha256").update(bytes).digest("hex");
    const release = join(directory, ".incodex/helpers/macos-device-key", digest);
    mkdirSync(release, { recursive: true, mode: 0o700 });
    writeFileSync(join(release, "incodex-device-key-helper"), bytes, { mode: 0o700 });
    const namespace = join(directory, ".incodex/macos-device-key");
    mkdirSync(namespace, { recursive: true, mode: 0o700 });
    writeFileSync(join(namespace, "registration.json"), JSON.stringify({ schemaVersion: 1, protocolVersion: 1, helperSha256: digest }), { mode: 0o600 });
    for (const part of [".incodex", ".incodex/helpers", ".incodex/helpers/macos-device-key", ".incodex/macos-device-key"]) chmodSync(join(directory, part), 0o700);
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
