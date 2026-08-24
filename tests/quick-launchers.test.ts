/**
 * [INPUT]: 依赖 scripts/setup-quick-launchers.sh 的本地生成器，依赖 macOS unzip 读取 Alfred 导入包
 * [OUTPUT]: 验证 Raycast/Alfred 产物的幂等性、官方导入边界、所有权与发布事务契约
 * [POS]: tests 的 Quick Launchers 产品契约，阻止生成器写入 Raycast/Alfred 私有配置或暴露不支持的卸载路由
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { describe, expect, test } from "bun:test";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

const repo = join(import.meta.dir, "..");
const setupScript = join(repo, "scripts", "setup-quick-launchers.sh");

function writeExecutable(path: string, body: string): void {
  writeFileSync(path, body);
  chmodSync(path, 0o755);
}

function fixture() {
  const home = mkdtempSync(join(tmpdir(), "incodex-launchers-home-"));
  const root = join(home, "owned launchers");
  const raycast = join(home, "Library", "Application Support", "Raycast", "script-commands");
  const fakeBin = join(home, "fake bin ' &");
  mkdirSync(fakeBin, { recursive: true });
  writeExecutable(join(fakeBin, "incodex"), "#!/bin/sh\nprintf '%s\\n' \"incodex:$*\"\n");
  return { home, root, raycast, fakeBin };
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

function runGenerated(
  context: ReturnType<typeof fixture>,
  script: string,
  extraEnv: Record<string, string> = {},
) {
  return spawnSync("/bin/bash", [script], {
    encoding: "utf8",
    env: {
      ...process.env,
      HOME: context.home,
      PATH: `${context.fakeBin}:/usr/bin:/bin`,
      ...extraEnv,
    },
  });
}

function ownedArtifacts(context: ReturnType<typeof fixture>): string[] {
  return [
    join(context.raycast, "incodex-open.sh"),
    join(context.raycast, "incodex-status.sh"),
    join(context.raycast, "incodex-doctor.sh"),
    join(context.root, "alfred", "Incodex Quick Launchers.alfredworkflow"),
    join(context.root, "manifest.sha256"),
    join(context.root, ".incodex-quick-launchers"),
  ];
}

function failOnceMovingRaycastStatus(binDir: string): void {
  writeExecutable(
    join(binDir, "mv"),
    `#!/bin/sh
if [ "$3" = "$HOME/Library/Application Support/Raycast/script-commands/incodex-status.sh" ] && [ ! -e "$HOME/mv-failed" ]; then
    /usr/bin/touch "$HOME/mv-failed"
    exit 75
fi
exec /bin/mv "$@"
`,
  );
}

function terminateOnceMovingRaycastStatus(binDir: string): void {
  writeExecutable(
    join(binDir, "mv"),
    `#!/bin/sh
if [ "$3" = "$HOME/Library/Application Support/Raycast/script-commands/incodex-status.sh" ] && [ ! -e "$HOME/mv-terminated" ]; then
    /usr/bin/touch "$HOME/mv-terminated"
    kill -TERM "$PPID"
    exit 75
fi
exec /bin/mv "$@"
`,
  );
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

    const raycast = context.raycast;
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
    expect(runner).toContain("INCODEX_LAUNCHER_APP");
    for (const terminal of [
      "Terminal",
      "iTerm2",
      "Alacritty",
      "kitty",
      "WezTerm",
      "Ghostty",
      "Hyper",
      "WindTerm",
      "Warp",
    ]) {
      expect(runner).toContain(terminal);
    }

    const second = runSetup(context);
    expect(second.status).toBe(0);
    const listed = spawnSync("/usr/bin/find", [context.home, "-type", "f"], { encoding: "utf8" });
    expect(Array.from(listed.stdout.match(/incodex-(?:open|status|doctor)\.sh/g) ?? []).sort()).toEqual(
      expectedScripts,
    );
    expect(listed.stdout.match(/\.alfredworkflow/g)?.length).toBe(1);
  });

  test("Raycast status and doctor use TERM directly or route through a terminal fallback", () => {
    const context = fixture();
    expect(runSetup(context).status).toBe(0);
    const termGuard = 'if [[ -n "' + "$" + '{TERM:-}" && "' + "$" + '{TERM}" != "dumb" ]]';

    for (const command of ["status", "doctor"]) {
      const source = readFileSync(join(context.raycast, `incodex-${command}.sh`), "utf8");
      expect(source).toContain(termGuard);
      expect(source).toContain('"$INCODEX_BIN"');
      expect(source).toContain('TERM_APP="$(detect_launcher_app)"');
      expect(source).toContain('launch_with_app "$TERM_APP"');
      expect(source).toContain('if [[ "$TERM_APP" != "Terminal" ]]');
      expect(source).toContain('launch_with_app "Terminal"');
    }
  });

  test("Raycast routes unavailable TERM through an app and falls back to Terminal", () => {
    const context = fixture();
    expect(runSetup(context).status).toBe(0);
    const script = join(context.raycast, "incodex-status.sh");

    const direct = runGenerated(context, script, { TERM: "xterm-256color" });
    expect(direct.status).toBe(0);
    expect(direct.stdout).toContain("incodex:status");

    mkdirSync(join(context.home, "Applications", "Hyper.app"), { recursive: true });
    writeExecutable(
      join(context.fakeBin, "open"),
      "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/open-args\"\nexit 1\n",
    );
    writeExecutable(
      join(context.fakeBin, "osascript"),
      "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/osascript-args\"\ncat > \"$HOME/osascript-script\"\nexit 0\n",
    );

    const fallback = runGenerated(context, script, {
      TERM: "dumb",
      INCODEX_LAUNCHER_APP: "Hyper",
    });
    expect(fallback.status).toBe(0);
    expect(readFileSync(join(context.home, "open-args"), "utf8")).toContain("Hyper");
    expect(readFileSync(join(context.home, "osascript-script"), "utf8")).toContain(
      'tell application "Terminal"',
    );

    const unavailable = runGenerated(context, script, {
      TERM: "dumb",
      INCODEX_LAUNCHER_APP: "MissingTerminal",
    });
    expect(unavailable.status).toBe(0);
    expect(readFileSync(join(context.home, "osascript-script"), "utf8")).toContain(
      'tell application "Terminal"',
    );
  });

  test("Raycast sends terminal-specific arguments to Hyper, WindTerm, and Warp", () => {
    const context = fixture();
    expect(runSetup(context).status).toBe(0);
    const script = join(context.raycast, "incodex-status.sh");
    writeExecutable(
      join(context.fakeBin, "open"),
      "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/open-args\"\nexit 0\n",
    );

    for (const terminal of ["Hyper", "WindTerm", "Warp"]) {
      mkdirSync(join(context.home, "Applications", `${terminal}.app`), { recursive: true });
      const launched = runGenerated(context, script, {
        TERM: "dumb",
        INCODEX_LAUNCHER_APP: terminal,
      });
      expect(launched.status).toBe(0);
      const args = readFileSync(join(context.home, "open-args"), "utf8").split("\n");
      expect(args).toContain(terminal);
      expect(args).toContain("--args");
      expect(args).not.toContain("-e");
    }
  });

  test("Alfred uses terminal-specific Hyper, WindTerm, and Warp arguments", () => {
    const context = fixture();
    expect(runSetup(context).status).toBe(0);
    const workflow = join(context.root, "alfred", "Incodex Quick Launchers.alfredworkflow");
    const runner = unzipEntry(workflow, "run.sh");

    for (const terminal of ["Hyper", "WindTerm", "Warp"]) {
      expect(runner).toMatch(
        new RegExp(`${terminal}\\)[\\s\\S]*open -na "${terminal}" --args /bin/zsh -lc`),
      );
    }
    expect(runner).not.toContain("Alacritty|Ghostty|Hyper|WindTerm|Warp");
    expect(runner).toContain('if launch_with_app "Terminal"');
  });

  test("uses Raycast's documented shared script directory without writing provider preferences", () => {
    const source = readFileSync(setupScript, "utf8");
    expect(source).toContain("$HOME/Library/Application Support/Raycast/script-commands");
    expect(source).not.toContain("com.raycast.macos.plist");
    expect(source).not.toContain("Alfred.alfredpreferences");
    expect(source).not.toMatch(/(?:curl|wget).*(?:main|master)/);
  });

  test("does not expose a launcher uninstall route or public uninstall curl command", () => {
    const context = fixture();
    expect(runSetup(context).status).toBe(0);
    const rejected = runSetup(context, ["uninstall"]);
    expect(rejected.status).not.toBe(0);
    expect(existsSync(join(context.raycast, "incodex-open.sh"))).toBe(true);
    expect(rejected.stdout + rejected.stderr).toContain("usage");

    const source = readFileSync(setupScript, "utf8");
    expect(source).not.toContain("uninstall");
    for (const readme of ["README.md", "README_CN.md"]) {
      expect(readFileSync(join(repo, readme), "utf8")).not.toMatch(/\| bash -s -- uninstall\b/);
    }
  });

  test("generated Raycast wrappers execute the quoted binary and reject a missing one", () => {
    const context = fixture();
    expect(runSetup(context).status).toBe(0);
    const raycast = context.raycast;
    const wrapperEnv = {
      ...process.env,
      HOME: context.home,
      PATH: `${context.fakeBin}:/usr/bin:/bin`,
      TERM: "xterm",
    };

    const status = spawnSync("/bin/bash", [join(raycast, "incodex-status.sh")], {
      encoding: "utf8",
      env: wrapperEnv,
    });
    expect(status.status).toBe(0);
    expect(status.stdout).toContain("incodex:status");

    unlinkSync(join(context.fakeBin, "incodex"));
    const open = spawnSync("/bin/bash", [join(raycast, "incodex-open.sh")], {
      encoding: "utf8",
      env: wrapperEnv,
    });
    expect(open.status).not.toBe(0);
    expect(open.stdout + open.stderr).toContain("no longer executable");
  });

  test("documents the optional launcher setup in both public readmes", () => {
    for (const readme of ["README.md", "README_CN.md"]) {
      const source = readFileSync(join(repo, readme), "utf8");
      expect(source).toMatch(
        /curl -fsSL https:\/\/raw\.githubusercontent\.com\/daftAI2026\/incodex\/main\/scripts\/setup-quick-launchers\.sh \| bash/,
      );
      expect(source).not.toMatch(/incodex\/[0-9a-f]{40}\/scripts\/setup-quick-launchers\.sh/);
      expect(source).toContain("Raycast");
      expect(source).toContain("Alfred");
      expect(source).toContain("INCODEX_LAUNCHER_APP");
      expect(source).toContain("Raycast v2");
      expect(source).toContain("Raycast v1");
      expect(source).toContain("Settings → Script Commands");
      expect(source).toContain("Script Folders");
      expect(source).toContain("~/Library/Application Support/Raycast/script-commands");
    }
  });

  test("documents Raycast TERM routing and Terminal fallback truthfully", () => {
    const english = readFileSync(join(repo, "README.md"), "utf8");
    const chinese = readFileSync(join(repo, "README_CN.md"), "utf8");
    expect(english).toContain("When Raycast provides a usable `TERM`, Status and Doctor run directly in its `fullOutput` pane");
    expect(english).toContain("falls back to Terminal");
    expect(chinese).toContain("Raycast 提供可用的 `TERM` 时，Status 和 Doctor 直接在它的 `fullOutput` 中运行");
    expect(chinese).toContain("回退到 Terminal");
  });

  test("a generation failure does not publish ownership or partial launchers", () => {
    const context = fixture();
    const alfred = join(context.root, "alfred");
    mkdirSync(alfred, { recursive: true });
    chmodSync(alfred, 0o500);

    const installed = runSetup(context);
    expect(installed.status).not.toBe(0);
    expect(existsSync(join(context.root, ".incodex-quick-launchers"))).toBe(false);
    expect(existsSync(join(context.raycast, "incodex-open.sh"))).toBe(false);
    chmodSync(alfred, 0o700);
  });

  test("a fresh publication failure leaves no launcher residue", () => {
    const context = fixture();
    failOnceMovingRaycastStatus(context.fakeBin);

    const installed = runSetup(context);
    expect(installed.status).not.toBe(0);
    for (const artifact of ownedArtifacts(context)) {
      expect(existsSync(artifact)).toBe(false);
    }
  });

  test("a reinstall publication failure restores the previous complete collection", () => {
    const context = fixture();
    expect(runSetup(context).status).toBe(0);
    const before = ownedArtifacts(context).map((artifact) => readFileSync(artifact));

    const secondBin = join(context.home, "second-bin");
    mkdirSync(secondBin, { recursive: true });
    writeExecutable(join(secondBin, "incodex"), "#!/bin/sh\nprintf '%s\\n' \"incodex-v2:$*\"\n");
    failOnceMovingRaycastStatus(secondBin);

    const installed = runSetup(context, [], {
      PATH: `${secondBin}:${context.fakeBin}:/usr/bin:/bin`,
    });
    expect(installed.status).not.toBe(0);
    expect(ownedArtifacts(context).map((artifact) => readFileSync(artifact))).toEqual(before);
  });

  test("a SIGTERM during fresh publication leaves no launcher residue", () => {
    const context = fixture();
    terminateOnceMovingRaycastStatus(context.fakeBin);

    const installed = runSetup(context);
    expect(installed.status).not.toBe(0);
    for (const artifact of ownedArtifacts(context)) {
      expect(existsSync(artifact)).toBe(false);
    }
  });

  test("a SIGTERM during reinstall restores the previous complete collection", () => {
    const context = fixture();
    expect(runSetup(context).status).toBe(0);
    const before = ownedArtifacts(context).map((artifact) => readFileSync(artifact));

    const secondBin = join(context.home, "second-bin");
    mkdirSync(secondBin, { recursive: true });
    writeExecutable(join(secondBin, "incodex"), "#!/bin/sh\nprintf '%s\\n' \"incodex-v2:$*\"\n");
    terminateOnceMovingRaycastStatus(secondBin);

    const installed = runSetup(context, [], {
      PATH: `${secondBin}:${context.fakeBin}:/usr/bin:/bin`,
    });
    expect(installed.status).not.toBe(0);
    expect(ownedArtifacts(context).map((artifact) => readFileSync(artifact))).toEqual(before);
  });

  test("install refuses fixed-name Raycast files it does not own without claiming the root", () => {
    const context = fixture();
    const raycast = context.raycast;
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

  test("reinstall refuses a tampered ownership manifest", () => {
    const context = fixture();
    expect(runSetup(context).status).toBe(0);
    const launcher = join(context.raycast, "incodex-open.sh");
    writeFileSync(launcher, "foreign launcher\n");
    const hash = spawnSync("/usr/bin/shasum", ["-a", "256", launcher], { encoding: "utf8" }).stdout
      .trim()
      .split(/\s+/)[0];
    const manifest = join(context.root, "manifest.sha256");
    const tampered = readFileSync(manifest, "utf8").replace(
      /^.*\traycast-open$/m,
      `${hash}\traycast-open`,
    );
    writeFileSync(manifest, tampered);

    const installed = runSetup(context);
    expect(installed.status).not.toBe(0);
    expect(readFileSync(launcher, "utf8")).toBe("foreign launcher\n");
    expect(installed.stdout + installed.stderr).toContain("manifest");
  });

  test("install refuses a symlink in a launcher root parent", () => {
    const context = fixture();
    const victim = mkdtempSync(join(tmpdir(), "incodex-launchers-parent-victim-"));
    const redirectedParent = join(context.home, "redirected-parent");
    symlinkSync(victim, redirectedParent);
    const redirectedRoot = join(redirectedParent, "quick-launchers");

    const installed = runSetup(context, [], {
      INCODEX_QUICK_LAUNCHERS_ROOT: redirectedRoot,
    });
    expect(installed.status).not.toBe(0);
    expect(existsSync(join(victim, "quick-launchers"))).toBe(false);
    expect(installed.stdout + installed.stderr).toMatch(/symlink|redirect/i);
  });

  test("install refuses symlinked ownership and provider directories", () => {
    for (const redirected of ["root", "raycast", "alfred"] as const) {
      const context = fixture();
      const victim = mkdtempSync(join(tmpdir(), `incodex-launchers-${redirected}-victim-`));
      if (redirected === "root") {
        symlinkSync(victim, context.root);
      } else if (redirected === "raycast") {
        mkdirSync(dirname(context.raycast), { recursive: true });
        symlinkSync(victim, context.raycast);
      } else {
        mkdirSync(context.root, { recursive: true });
        symlinkSync(victim, join(context.root, "alfred"));
      }

      const installed = runSetup(context);
      expect(installed.status).not.toBe(0);
      expect(existsSync(join(victim, ".incodex-quick-launchers"))).toBe(false);
      expect(existsSync(join(victim, "incodex-open.sh"))).toBe(false);
      expect(existsSync(join(victim, "Incodex Quick Launchers.alfredworkflow"))).toBe(false);
    }
  });
});
