/**
 * [INPUT]: 依赖 runtime-artifacts.json、冻结 CLI 证据和官方二进制身份。
 * [OUTPUT]: 校验候选资产、代际 ACK、IPC 结果和精确目标会话路径。
 * [POS]: 验收入口和宿主适配器共用的纯边界；不启动 App，也不查询 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { createHash } from "node:crypto";
import { closeSync, constants as fsConstants, fstatSync, lstatSync, openSync, readFileSync, realpathSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

export type CandidateEvidence = {
  schemaVersion: number;
  candidateId: string;
  cliVersion: string;
  cliSha256: string;
  runtimeVersion: string;
  runtimeManifestSha256: string;
  runtimeSourceManifestSha256: string;
  runtimeFiles: Record<string, string>;
  hotAcceptance?: {
    kind: string;
    action: string;
    artifact: string;
    code: string;
    reason: string;
    artifactSha256: string;
  };
  [key: string]: unknown;
};
export type Mode = "run" | "self-test";
export type Options = {
  mode: Mode;
  aCli?: string;
  aEvidence?: string;
  bCli?: string;
  bEvidence?: string;
  sourceCodexHome?: string;
  output?: string;
};
export type ActionExpectation = { ok: boolean; code: string; reason?: string };
export type AckExpectation = {
  pid: number;
  processStartIdentity: string;
  manifestSha256: string;
  actionSha256: string;
  uiSha256: string;
  windowId: number;
};
export type RuntimePointer = {
  schemaVersion: number;
  version: string;
  sourceCommit?: string;
  release: string;
  manifestSha256: string;
  files: Record<string, string>;
};
export type RuntimeAck = {
  phase?: string;
  pid?: number;
  processStartIdentity?: string | null;
  published?: { generation?: string | null } | null;
  main?: { generation?: string | null; artifact?: string | null } | null;
  restartRequired?: boolean;
  installRequired?: boolean;
  rollbackFailed?: boolean;
  renderers?: Array<{
    windowId?: number;
    generation?: string | null;
    ui?: string | null;
    state?: string;
  }>;
};
export type Report = {
  schemaVersion: 1;
  startedAt: string;
  candidateA: string;
  candidateB: string;
  host: { pid: number | null; processStartIdentity: string | null; windowId: number | null };
  proof: Record<string, boolean>;
  events: Array<Record<string, string | number | boolean | null>>;
  result: "running" | "passed" | "failed";
  failure: string | null;
  restoreA: "not-needed" | "restored" | "failed";
  hostClose: "not-started" | "window-closed" | "window-close-failed";
};

export const PRODUCT_APP = "/Applications/ChatGPT.app";
export const PRODUCT_BINARY = `${PRODUCT_APP}/Contents/MacOS/ChatGPT`;
export const PRODUCT_BUNDLE_ID = "com.openai.codex";
export const ACTION_ARTIFACT = "incodex-main-actions.cjs";
const NATIVE_RUNTIME_MANIFEST = "runtime-native-manifest.json";
const RUNTIME_ARTIFACT_CATALOG = JSON.parse(
  readFileSync(new URL("../runtime-artifacts.json", import.meta.url), "utf8"),
) as { loader: string; external: string[] };
const NATIVE_RUNTIME_ARTIFACTS = new Set([
  "incodex-permission-host",
  "incodex-permission-ui.dylib",
  "incodex-remote-key-compat.node",
  "incodex-remote-key-manifest.json",
  NATIVE_RUNTIME_MANIFEST,
]);
const CURRENT_RUNTIME_ARTIFACTS = new Set([
  ...RUNTIME_ARTIFACT_CATALOG.external,
  ...NATIVE_RUNTIME_ARTIFACTS,
]);
export const RUNTIME_ROOT = join(homedir(), ".incodex", "runtime");
export const USER_ROOT = join(homedir(), ".incodex");
export const SESSIONS_ROOT = join(USER_ROOT, "sessions");
export const TARGET_ID = createHash("sha256").update(PRODUCT_BINARY).digest("hex").slice(0, 12);
export const TARGET_SESSIONS_ROOT = join(SESSIONS_ROOT, TARGET_ID);
export const TARGET_STATE = join(USER_ROOT, "targets", TARGET_ID);
export const ACK_LOG = join(TARGET_STATE, "logs", "incognito.log");
export const SESSION_ID = /^s-[A-Za-z0-9]+$/u;
export const SHA256 = /^[0-9a-f]{64}$/u;
export const MAX_JSON_BYTES = 2 * 1024 * 1024;
export const MAX_LOG_BYTES = 8 * 1024 * 1024;
export const STARTUP_TIMEOUT_MS = 90_000;
export const ACK_TIMEOUT_MS = 45_000;
export const CHILD_TIMEOUT_MS = 60_000;
export const CLOSE_TIMEOUT_MS = 30_000;
export const POLL_MS = 200;

export function usage(): string {
  return [
    "Explicit macOS Runtime A→B→A acceptance. This command can publish Runtime and open windows.",
    "It refuses to run without --run and requires a new output directory under /private/tmp.",
    "  bun scripts/incodex-macos-runtime-hot-acceptance.ts --self-test",
    "  bun scripts/incodex-macos-runtime-hot-acceptance.ts --run --a-cli /abs/incodex-a --a-evidence /abs/a.json --b-cli /abs/incodex-b --b-evidence /abs/b.json --out /private/tmp/new-hot-acceptance",
    "Optional: --source-codex-home /absolute/path/to/CODEX_HOME (copies only auth.json and config.toml).",
    "Before run, the operator confirms Accessibility and Screen Recording permission; this script does not query TCC.",
    "Other preconditions: no running official Codex host, current Runtime is A, official loader-only install is valid, and the exact B action artifact differs only in incodex-main-actions.cjs.",
    "B proof uses the production open result: reason hot-acceptance-b maps to code HOT-ACCEPTANCE-B.",
    "The run preserves HOME and Keychain. It isolates only CODEX_HOME and Chromium userData. It never kills or quits an official host.",
  ].join("\n");
}
export function parseArguments(args: string[]): Options {
  let mode: Mode | null = null;
  const values: Record<string, string> = {};
  const allowed = new Set(["--a-cli", "--a-evidence", "--b-cli", "--b-evidence", "--source-codex-home", "--out"]);
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index]!;
    if (arg === "--run" || arg === "--self-test") {
      if (mode) throw new Error("choose exactly one of --run or --self-test");
      mode = arg === "--run" ? "run" : "self-test";
      continue;
    }
    if (arg === "--help" || arg === "-h") throw new Error(usage());
    if (!allowed.has(arg)) throw new Error("unknown argument");
    if (values[arg]) throw new Error(`${arg} may be provided only once`);
    const value = args[index + 1];
    if (!value || value.startsWith("--")) throw new Error(`${arg} requires a value`);
    values[arg] = value;
    index += 1;
  }
  if (!mode) throw new Error("--run is required; use --self-test for pure checks");
  if (mode === "self-test") {
    if (Object.keys(values).length) throw new Error("--self-test does not accept run options");
    return { mode };
  }
  for (const flag of ["--a-cli", "--a-evidence", "--b-cli", "--b-evidence", "--out"]) {
    if (!values[flag] || !isAbsolute(values[flag]!)) throw new Error(`--run requires ${flag} with an absolute path`);
  }
  if (values["--source-codex-home"] && !isAbsolute(values["--source-codex-home"]!)) {
    throw new Error("--source-codex-home must be absolute");
  }
  const output = resolve(values["--out"]!);
  if (dirname(output) !== "/private/tmp" || output === "/private/tmp") {
    throw new Error("--out must name a new direct child of /private/tmp");
  }
  return {
    mode,
    aCli: values["--a-cli"],
    aEvidence: values["--a-evidence"],
    bCli: values["--b-cli"],
    bEvidence: values["--b-evidence"],
    sourceCodexHome: values["--source-codex-home"],
    output,
  };
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

export function isHash(value: unknown): value is string {
  return typeof value === "string" && SHA256.test(value);
}

export function isRuntimeArtifactName(name: string): boolean {
  return CURRENT_RUNTIME_ARTIFACTS.has(name);
}

export function matchesExternalRuntimeManifest(
  pointerFiles: Record<string, unknown>,
  manifestFiles: Record<string, unknown>,
): boolean {
  if (!isHash(manifestFiles[RUNTIME_ARTIFACT_CATALOG.loader])) return false;
  const externalFiles = Object.entries(manifestFiles).filter(([name]) => name !== RUNTIME_ARTIFACT_CATALOG.loader);
  return Object.keys(pointerFiles).length === externalFiles.length &&
    externalFiles.every(([name, digest]) => isRuntimeArtifactName(name) && pointerFiles[name] === digest);
}

function validateEvidence(value: unknown, label: string): CandidateEvidence {
  if (!isRecord(value) || value.schemaVersion !== 1 || typeof value.candidateId !== "string" ||
      !/^[A-Za-z0-9][A-Za-z0-9._-]{0,95}$/u.test(value.candidateId) ||
      typeof value.cliVersion !== "string" || !value.cliVersion || !isHash(value.cliSha256) ||
      typeof value.runtimeVersion !== "string" || !value.runtimeVersion ||
      !isHash(value.runtimeManifestSha256) || !isHash(value.runtimeSourceManifestSha256) ||
      !isRecord(value.runtimeFiles)) {
    throw new Error(`${label} frozen evidence is incomplete`);
  }
  const files: Record<string, string> = {};
  for (const [name, digest] of Object.entries(value.runtimeFiles)) {
    if (!isRuntimeArtifactName(name)) {
      throw new Error(`${label} evidence contains an unsupported Runtime artifact name`);
    }
    if (!isHash(digest)) throw new Error(`${label} evidence contains an invalid Runtime artifact digest`);
    files[name] = digest;
  }
  for (const required of ["incodex-inject.js", ACTION_ARTIFACT, "incodex-main.cjs", "incodex-preload.cjs"]) {
    if (!files[required]) throw new Error(`${label} evidence is missing a required Runtime artifact`);
  }
  return { ...value, runtimeFiles: files } as CandidateEvidence;
}

export function assertPairEvidence(pair: { a: unknown; b: unknown }): {
  aId: string; bId: string; action: "open"; code: "HOT-ACCEPTANCE-B"; reason: "hot-acceptance-b";
} {
  const a = validateEvidence(pair.a, "A");
  const b = validateEvidence(pair.b, "B");
  if (a.candidateId === b.candidateId || a.cliSha256 === b.cliSha256 ||
      a.runtimeManifestSha256 === b.runtimeManifestSha256 || a.runtimeFiles[ACTION_ARTIFACT] === b.runtimeFiles[ACTION_ARTIFACT]) {
    throw new Error("A and B must identify distinct frozen CLI and Runtime generations");
  }
  if (a.cliVersion !== b.cliVersion || a.runtimeVersion !== b.runtimeVersion) {
    throw new Error("A and B must use one CLI and Runtime version for a controlled hot action change");
  }
  const names = new Set([...Object.keys(a.runtimeFiles), ...Object.keys(b.runtimeFiles)]);
  const changed = [...names].filter((name) => a.runtimeFiles[name] !== b.runtimeFiles[name]);
  if (changed.length !== 1 || changed[0] !== ACTION_ARTIFACT) {
    throw new Error("A and B may differ only in incodex-main-actions.cjs");
  }
  const proof = b.hotAcceptance;
  if (proof?.kind !== "bound-open-denial" || proof.action !== "open" || proof.artifact !== ACTION_ARTIFACT ||
      proof.code !== "HOT-ACCEPTANCE-B" || proof.reason !== "hot-acceptance-b" ||
      proof.artifactSha256 !== b.runtimeFiles[ACTION_ARTIFACT]) {
    throw new Error("B action proof is missing or does not match the frozen production artifact");
  }
  return { aId: a.candidateId, bId: b.candidateId, action: "open", code: "HOT-ACCEPTANCE-B", reason: "hot-acceptance-b" };
}

export function acceptedRuntimeAck(snapshot: unknown, expected: AckExpectation): boolean {
  if (!isRecord(snapshot)) return false;
  const value = snapshot as RuntimeAck;
  if (value.phase !== "active" || value.pid !== expected.pid ||
      value.processStartIdentity !== expected.processStartIdentity || value.restartRequired !== false ||
      value.installRequired !== false || value.rollbackFailed !== false ||
      value.published?.generation !== expected.manifestSha256 ||
      value.main?.generation !== expected.manifestSha256 || value.main?.artifact !== expected.actionSha256) return false;
  const renderers = value.renderers?.filter((row) => row.windowId === expected.windowId) ?? [];
  return renderers.length === 1 && renderers[0]?.generation === expected.manifestSha256 &&
    renderers[0]?.ui === expected.uiSha256 && renderers[0]?.state === "acknowledged";
}

export function verifyActionResult(actual: unknown, expected: ActionExpectation): void {
  if (!isRecord(actual) || actual.ok !== expected.ok || actual.code !== expected.code ||
      (expected.reason !== undefined && actual.reason !== expected.reason)) {
    throw new Error("action result mismatch");
  }
}

function sha256(bytes: Uint8Array): string {
  return createHash("sha256").update(bytes).digest("hex");
}

function readRegularJson(path: string, label: string, privateFile = false): unknown {
  const before = lstatSync(path);
  if (before.isSymbolicLink() || !before.isFile() || before.size > MAX_JSON_BYTES ||
      (privateFile && (before.mode & 0o022) !== 0) ||
      (typeof process.getuid === "function" && before.uid !== process.getuid())) {
    throw new Error(`${label} must be a private regular file`);
  }
  const fd = openSync(path, fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW || 0));
  try {
    const after = fstatSync(fd);
    if (!after.isFile() || after.dev !== before.dev || after.ino !== before.ino || after.size > MAX_JSON_BYTES) {
      throw new Error(`${label} changed during the bounded read`);
    }
    return JSON.parse(readFileSync(fd, "utf8"));
  } finally { closeSync(fd); }
}

export function readCandidate(path: string, label: string): CandidateEvidence {
  return validateEvidence(readRegularJson(path, `${label} evidence`, true), label);
}

export function requireExecutable(path: string, evidence: CandidateEvidence, label: string): string {
  const stat = lstatSync(path);
  if (stat.isSymbolicLink() || !stat.isFile() || (stat.mode & 0o111) === 0 || (stat.mode & 0o022) !== 0) {
    throw new Error(`${label} CLI must be a non-writable regular executable`);
  }
  const real = realpathSync(path);
  if (sha256(readFileSync(real)) !== evidence.cliSha256) throw new Error(`${label} CLI digest does not match frozen evidence`);
  return real;
}

export function runReadOnly(command: string, args: string[], timeout = 30_000): string {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    timeout,
    maxBuffer: 4 * 1024 * 1024,
    env: acceptanceCommandEnvironment(),
  });
  if (result.error || result.status !== 0) throw new Error("read-only preflight command failed");
  return result.stdout;
}

export function runCli(cli: string, args: string[], label: string, timeout = 120_000): string {
  const result = spawnSync(cli, args, {
    encoding: "utf8",
    timeout,
    maxBuffer: 4 * 1024 * 1024,
    env: acceptanceCommandEnvironment(),
  });
  if (result.error || result.status !== 0) throw new Error(`${label} CLI command failed`);
  return result.stdout;
}

export function verifyCliVersion(output: string, expected: string, label: string): void {
  const match = output.match(/^Incodex version ([^\r\n]+)$/mu);
  if (!match || match[1] !== expected) throw new Error(`${label} CLI version does not match frozen evidence`);
}

function acceptanceCommandEnvironment(): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = { ...process.env, LC_ALL: "C", NO_COLOR: "1" };
  for (const name of Object.keys(env)) if (name.startsWith("INCODEX_")) delete env[name];
  return env;
}

type StatusReport = {
  target?: string;
  patched?: boolean;
  bundleId?: string;
  asarLoaderOnly?: boolean | null;
  externalRuntime?: {
    present?: boolean;
    ok?: boolean;
    state?: string;
    version?: string;
    bundledVersion?: string;
    bundledManifestSha256?: string;
    manifestSha256?: string;
    matchesBundled?: boolean | null;
  };
};

export function readCandidateStatus(cli: string, label: string): StatusReport {
  let value: StatusReport;
  try { value = JSON.parse(runReadOnly(cli, ["status", "--json"])); }
  catch { throw new Error(`${label} status is not valid JSON`); }
  if (!isRecord(value) || value.target !== PRODUCT_APP || value.bundleId !== PRODUCT_BUNDLE_ID ||
      value.patched !== true || value.asarLoaderOnly !== true || !isRecord(value.externalRuntime)) {
    throw new Error(`${label} CLI does not report the fixed loader-only official install`);
  }
  return value;
}

export function requireBundledIdentity(status: StatusReport, evidence: CandidateEvidence, label: string): void {
  const runtime = status.externalRuntime;
  if (runtime?.bundledVersion !== evidence.runtimeVersion ||
      runtime.bundledManifestSha256 !== evidence.runtimeManifestSha256) {
    throw new Error(`${label} CLI embedded Runtime does not match frozen manifest evidence`);
  }
}

export function readCurrentPointer(): { pointer: RuntimePointer; files: Record<string, string> } {
  const pointerPath = join(RUNTIME_ROOT, "current.json");
  const pointerStat = lstatSync(pointerPath);
  if (pointerStat.isSymbolicLink() || !pointerStat.isFile() || pointerStat.size > 64 * 1024) {
    throw new Error("current Runtime pointer is not a bounded regular file");
  }
  const pointer = readRegularJson(pointerPath, "current Runtime pointer") as RuntimePointer;
  if (!isRecord(pointer) || pointer.schemaVersion !== 1 || !isHash(pointer.manifestSha256) ||
      typeof pointer.version !== "string" || typeof pointer.release !== "string" ||
      !pointer.release.startsWith("releases/") || pointer.release.includes("..") ||
      !isRecord(pointer.files)) throw new Error("current Runtime pointer is invalid");
  const releaseDir = resolve(RUNTIME_ROOT, pointer.release);
  if (!releaseDir.startsWith(`${resolve(RUNTIME_ROOT)}/`) || realpathSync(releaseDir) !== releaseDir) {
    throw new Error("current Runtime release escaped its root");
  }
  const manifestPath = join(releaseDir, "runtime-manifest.json");
  const manifestStat = lstatSync(manifestPath);
  if (manifestStat.isSymbolicLink() || !manifestStat.isFile() || manifestStat.size > MAX_JSON_BYTES) {
    throw new Error("current Runtime manifest is not a bounded regular file");
  }
  const manifestBytes = readFileSync(manifestPath);
  if (sha256(manifestBytes) !== pointer.manifestSha256) throw new Error("current Runtime manifest digest mismatch");
  const manifest = JSON.parse(manifestBytes.toString("utf8"));
  if (!isRecord(manifest) || manifest.runtimeVersion !== pointer.version || !isRecord(manifest.files)) {
    throw new Error("current Runtime manifest identity mismatch");
  }
  const files: Record<string, string> = {};
  for (const [name, expected] of Object.entries(pointer.files)) {
    if (!isHash(expected) || manifest.files[name] !== expected || !isRuntimeArtifactName(name)) {
      throw new Error("current Runtime file map is invalid");
    }
    const file = join(releaseDir, name);
    const stat = lstatSync(file);
    if (stat.isSymbolicLink() || !stat.isFile() || stat.size > 16 * 1024 * 1024 || sha256(readFileSync(file)) !== expected) {
      throw new Error("current Runtime artifact digest mismatch");
    }
    files[name] = expected;
  }
  if (!matchesExternalRuntimeManifest(files, manifest.files)) {
    throw new Error("current Runtime pointer and manifest file maps differ");
  }
  return { pointer, files };
}

export function requireCurrentCandidate(current: ReturnType<typeof readCurrentPointer>, evidence: CandidateEvidence, label: string): void {
  if (current.pointer.version !== evidence.runtimeVersion ||
      current.pointer.manifestSha256 !== evidence.runtimeManifestSha256 ||
      Object.keys(current.files).length !== Object.keys(evidence.runtimeFiles).length ||
      Object.entries(evidence.runtimeFiles).some(([name, digest]) => current.files[name] !== digest)) {
    throw new Error(`${label} is not the verified current Runtime generation`);
  }
}

export function requirePrivateTmpOutput(output: string): void {
  try {
    lstatSync(output);
    throw new Error("--out already exists; refusing to reuse acceptance output");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }
  const parent = realpathSync("/private/tmp");
  if (parent !== "/private/tmp" || dirname(output) !== parent) throw new Error("/private/tmp is not the canonical output parent");
}
