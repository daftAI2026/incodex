/**
 * [INPUT]: 依赖合成宿主、真实隔离签名与临时 Keychain。
 * [OUTPUT]: 验证只读宿主身份门、密钥策略与错误所有权；签名失败附临时身份诊断。
 * [POS]: 兼容模块回归；只用隔离身份，不读用户私钥匙串。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { spawnSync } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "bun:test";
import { installRemoteKeyCompatibility } from "./incodex-remote-key-compat.cts";

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

test.skipIf(process.platform !== "darwin")("host identity verifies real local signatures without private assets or search-list changes", () => {
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
          const result = spawnSync("/usr/bin/codesign", args, { encoding: "utf8", timeout: 5_000 });
          return `${label}: exit=${result.status} ${(result.stderr ?? "").slice(0, 512)}`;
        }).join("\n");
        throw new Error(`${String(error)}\nTemporary signing fixture diagnostics:\n${diagnostics}\nSelector probes:\n${selectorProbes}`);
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
