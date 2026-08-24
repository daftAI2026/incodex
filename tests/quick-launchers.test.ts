/**
 * [INPUT]: 依赖 scripts/setup-quick-launchers.sh 的本地生成器，依赖 macOS unzip 读取 Alfred 导入包
 * [OUTPUT]: 验证 Raycast/Alfred 产物的幂等性、官方导入边界、所有权与安全卸载契约
 * [POS]: tests 的 Quick Launchers 产品契约，阻止生成器写入 Raycast/Alfred 私有配置或伪造 TTY 确认
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";

const repo = join(import.meta.dir, "..");
const setupScript = join(repo, "scripts", "setup-quick-launchers.sh");

function writeExecutable(path: string, body: string): void {
  writeFileSync(path, body);
  chmodSync(path, 0o755);
}

function fixture() {
  const home = mkdtempSync(join(tmpdir(), "incodex-launchers-home-"));
  const root = join(home, "owned launchers");
  const fakeBin = join(home, "fake bin");
  mkdirSync(fakeBin, { recursive: true });
  writeExecutable(join(fakeBin, "incodex"), "#!/bin/sh\nprintf '%s\\n' \"incodex:$*\"\n");
  return { home, root, fakeBin };
}

function runSetup(
  context: ReturnType<typeof fixture>,
  args: string[] = [],
  extraEnv: Record<string, string> = {},
) {
  return spawnSync("/bin/bash", [setupScript, ...args], {
    encoding: "utf8",
    env: {
      ...process.env,
      HOME: context.home,
      PATH: `${context.fakeBin}:/usr/bin:/bin`,
      INCODEX_QUICK_LAUNCHERS_ROOT: context.root,
      INCODEX_LAUNCHERS_NO_OPEN: "1",
      ...extraEnv,
    },
  });
}

function unzipEntry(archive: string, entry: string): string {
  const result = spawnSync("/usr/bin/unzip", ["-p", archive, entry], { encoding: "utf8" });
  if (result.status !== 0) throw new Error(result.stderr || `cannot read ${entry}`);
  return result.stdout;
}

describe("setup-quick-launchers.sh", () => {
  test("generates a stable Raycast directory and a standard Alfred import package", () => {
    const context = fixture();
    const first = runSetup(context);
    expect(first.status).toBe(0);

    const raycast = join(context.root, "raycast");
    const expectedScripts = ["incodex-doctor.sh", "incodex-open.sh", "incodex-status.sh"];
    for (const filename of expectedScripts) {
      expect(existsSync(join(raycast, filename))).toBe(true);
    }

    const openScript = readFileSync(join(raycast, "incodex-open.sh"), "utf8");
    expect(openScript).toContain("@raycast.schemaVersion 1");
    expect(openScript).toContain("@raycast.title Incodex Open");
    expect(openScript).toContain("@raycast.mode silent");
    expect(openScript).toContain("@raycast.packageName Incodex");
    expect(openScript).toContain("@raycast.platform macos");
    expect(openScript).toContain("nohup");
    expect(openScript).not.toContain("--yes");

    for (const command of ["status", "doctor"]) {
      const script = readFileSync(join(raycast, `incodex-${command}.sh`), "utf8");
      expect(script).toContain("@raycast.mode fullOutput");
      expect(script).toContain(`" ${command}`);
      expect(script).not.toContain("--yes");
    }

    const workflow = join(context.root, "alfred", "Incodex Quick Launchers.alfredworkflow");
    expect(existsSync(workflow)).toBe(true);
    const plist = unzipEntry(workflow, "info.plist");
    const runner = unzipEntry(workflow, "run.sh");
    expect(plist).toContain("com.daftai.incodex.quick-launchers");
    expect(plist).toContain("incognito");
    expect(plist).toContain("inc-status");
    expect(plist).toContain("inc-doctor");
    expect(plist).toContain("incodex.quick.open.input");
    expect(plist).toContain("incodex.quick.doctor.action");
    expect(runner).not.toContain("--yes");
    expect(runner).not.toContain("Alfred.alfredpreferences");

    const second = runSetup(context);
    expect(second.status).toBe(0);
    const listed = spawnSync("/usr/bin/find", [context.root, "-type", "f"], { encoding: "utf8" });
    expect(listed.stdout.match(/incodex-(?:open|status|doctor)\.sh/g)?.sort()).toEqual(expectedScripts);
    expect(listed.stdout.match(/\.alfredworkflow/g)?.length).toBe(1);
  });

  test("never writes launcher files into Raycast or Alfred private preferences", () => {
    const source = readFileSync(setupScript, "utf8");
    expect(source).not.toContain("Application Support/Raycast");
    expect(source).not.toContain("Alfred.alfredpreferences");
    expect(source).not.toMatch(/(?:curl|wget).*(?:main|master)/);
  });

  test("uninstall removes only marker-owned artifacts and leaves user files alone", () => {
    const context = fixture();
    expect(runSetup(context).status).toBe(0);
    const userFile = join(context.root, "raycast", "mine.sh");
    writeFileSync(userFile, "#!/bin/sh\necho mine\n");

    const removed = runSetup(context, ["uninstall"]);
    expect(removed.status).toBe(0);
    expect(existsSync(userFile)).toBe(true);
    expect(existsSync(join(context.root, "raycast", "incodex-open.sh"))).toBe(false);
    expect(existsSync(join(context.root, "alfred", "Incodex Quick Launchers.alfredworkflow"))).toBe(false);
    expect(removed.stdout + removed.stderr).toContain("Alfred Preferences");
  });

  test("uninstall fails closed when the ownership marker changed", () => {
    const context = fixture();
    expect(runSetup(context).status).toBe(0);
    writeFileSync(join(context.root, ".incodex-quick-launchers"), "foreign owner\n");

    const removed = runSetup(context, ["uninstall"]);
    expect(removed.status).not.toBe(0);
    expect(existsSync(join(context.root, "raycast", "incodex-open.sh"))).toBe(true);
    expect(removed.stdout + removed.stderr).toContain("ownership marker");
  });

  test("install refuses fixed-name Raycast files it does not own without claiming the root", () => {
    const context = fixture();
    const raycast = join(context.root, "raycast");
    const foreign = join(raycast, "incodex-open.sh");
    mkdirSync(raycast, { recursive: true });
    writeFileSync(foreign, "#!/bin/sh\necho foreign\n");

    const installed = runSetup(context);
    expect(installed.status).not.toBe(0);
    expect(readFileSync(foreign, "utf8")).toContain("foreign");
    expect(existsSync(join(context.root, ".incodex-quick-launchers"))).toBe(false);
  });

  test("reinstall refuses an Alfred package whose ownership proof is missing", () => {
    const context = fixture();
    expect(runSetup(context).status).toBe(0);
    const workflow = join(context.root, "alfred", "Incodex Quick Launchers.alfredworkflow");
    writeFileSync(workflow, "foreign workflow");

    const installed = runSetup(context);
    expect(installed.status).not.toBe(0);
    expect(readFileSync(workflow, "utf8")).toBe("foreign workflow");
  });
});
