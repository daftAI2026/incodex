import { describe, expect, test } from "bun:test";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

const repo = join(import.meta.dir, "..");
const setupScript = join(repo, "scripts", "setup-quick-launchers.sh");
const generatedMarker = "incodex-quick-launchers generated";

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

function enableAlfredApp(context: ReturnType<typeof fixture>): string {
  const app = join(context.home, "Applications", "Alfred 5.app");
  mkdirSync(app, { recursive: true });
  return app;
}

function paths(context: ReturnType<typeof fixture>) {
  return {
    runner: join(context.root, "runner.sh"),
    open: join(context.raycast, "incodex-open.sh"),
    status: join(context.raycast, "incodex-status.sh"),
    doctor: join(context.raycast, "incodex-doctor.sh"),
    workflow: join(context.root, "alfred", "Incodex Quick Launchers.alfredworkflow"),
  };
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

function runSetupAsync(context: ReturnType<typeof fixture>) {
  const child = spawn("/bin/bash", [setupScript], {
    env: {
      ...process.env,
      HOME: context.home,
      PATH: `${context.fakeBin}:/usr/bin:/bin`,
      INCODEX_QUICK_LAUNCHERS_ROOT: context.root,
      INCODEX_LAUNCHERS_NO_OPEN: "1",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  return new Promise<{ status: number | null; stdout: string; stderr: string }>((resolve, reject) => {
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk: Buffer) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk: Buffer) => {
      stderr += chunk.toString();
    });
    child.on("error", reject);
    child.on("close", (status) => resolve({ status, stdout, stderr }));
  });
}

function runGenerated(
  context: ReturnType<typeof fixture>,
  script: string,
  args: string[] = [],
  extraEnv: Record<string, string> = {},
) {
  return spawnSync("/bin/bash", [script, ...args], {
    encoding: "utf8",
    env: {
      ...process.env,
      HOME: context.home,
      PATH: `${context.fakeBin}:/usr/bin:/bin`,
      ...extraEnv,
    },
  });
}

function unzipEntry(archive: string, entry: string): string {
  const result = spawnSync("/usr/bin/unzip", ["-p", archive, entry], { encoding: "utf8" });
  if (result.status !== 0) throw new Error(result.stderr || `cannot read ${entry}`);
  return result.stdout;
}

function writeFailOnceStatusMove(binDir: string): void {
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

function assertThinRaycastWrapper(source: string, command: string): void {
  expect(source).toContain(`# ${generatedMarker}`);
  expect(source).toMatch(new RegExp(`^exec .*runner\\.sh ${command}$`, "m"));
  expect(source).not.toContain("INCODEX_BIN");
  expect(source).not.toContain("command -v");
  expect(source).not.toContain("TERM");
  expect(source).not.toContain("osascript");
}

describe("setup-quick-launchers.sh", () => {
  test("creates one dynamic runner and thin Raycast/Alfred wrappers", () => {
    const context = fixture();
    const installed = runSetup(context, [], { PATH: "/usr/bin:/bin" });
    expect(installed.status).toBe(0);

    const generated = paths(context);
    const runner = readFileSync(generated.runner, "utf8");
    expect(runner).toContain(`# ${generatedMarker}`);
    expect(runner).toContain("resolve_incodex");
    expect(runner).toContain('INCODEX_BIN="$(resolve_incodex)"');
    expect(runner).toContain("launch_in_terminal");

    assertThinRaycastWrapper(readFileSync(generated.open, "utf8"), "open");
    assertThinRaycastWrapper(readFileSync(generated.status, "utf8"), "status");
    assertThinRaycastWrapper(readFileSync(generated.doctor, "utf8"), "doctor");
    expect(installed.stdout).toContain("Alfred not detected; skipped");
    expect(existsSync(generated.workflow)).toBe(false);
    expect(existsSync(join(context.root, "alfred"))).toBe(false);
    expect(existsSync(join(context.root, "manifest.sha256"))).toBe(false);
    expect(existsSync(join(context.root, ".install.lock"))).toBe(false);
  });

  test("generates the standard Alfred package only when Alfred is detected", () => {
    const context = fixture();
    const app = enableAlfredApp(context);
    const installed = runSetup(context, [], { INCODEX_ALFRED_APP: app });
    expect(installed.status).toBe(0);
    const generated = paths(context);
    const alfredRunner = unzipEntry(generated.workflow, "run.sh");
    expect(alfredRunner).toContain(`# ${generatedMarker}`);
    expect(alfredRunner).toMatch(/^exec .*runner\.sh "\$\{1:-\}"$/m);
    expect(alfredRunner).not.toContain("INCODEX_BIN");
    expect(alfredRunner).not.toContain("command -v");
    expect(unzipEntry(generated.workflow, "info.plist")).toContain("com.daftai.incodex.quick-launchers");
    expect(installed.stdout).toContain("Alfred package:");

    const preferences = fixture();
    mkdirSync(join(preferences.home, "Library", "Application Support", "Alfred", "Alfred.alfredpreferences"), {
      recursive: true,
    });
    expect(runSetup(preferences).status).toBe(0);
    expect(existsSync(paths(preferences).workflow)).toBe(true);
  });

  test("provider content does not depend on the installation PATH", () => {
    const context = fixture();
    const app = enableAlfredApp(context);
    expect(runSetup(context, [], { PATH: "/usr/bin:/bin", INCODEX_ALFRED_APP: app }).status).toBe(0);
    const generated = paths(context);
    const before = {
      runner: readFileSync(generated.runner, "utf8"),
      open: readFileSync(generated.open, "utf8"),
      status: readFileSync(generated.status, "utf8"),
      doctor: readFileSync(generated.doctor, "utf8"),
      alfred: unzipEntry(generated.workflow, "run.sh"),
    };

    const secondBin = join(context.home, "second-bin");
    mkdirSync(secondBin, { recursive: true });
    writeExecutable(join(secondBin, "incodex"), "#!/bin/sh\nprintf '%s\\n' \"incodex-v2:$*\"\n");
    expect(runSetup(context, [], { PATH: `${secondBin}:/usr/bin:/bin`, INCODEX_ALFRED_APP: app }).status).toBe(0);

    expect(readFileSync(generated.runner, "utf8")).toBe(before.runner);
    expect(readFileSync(generated.open, "utf8")).toBe(before.open);
    expect(readFileSync(generated.status, "utf8")).toBe(before.status);
    expect(readFileSync(generated.doctor, "utf8")).toBe(before.doctor);
    expect(unzipEntry(generated.workflow, "run.sh")).toBe(before.alfred);
    for (const source of Object.values(before)) {
      expect(source).not.toContain(context.fakeBin);
      expect(source).not.toContain(secondBin);
    }
  });

  test("the runner resolves the current incodex binary at execution time", () => {
    const context = fixture();
    expect(runSetup(context, [], { PATH: "/usr/bin:/bin" }).status).toBe(0);
    const generated = paths(context);

    const first = runGenerated(context, generated.status, [], { TERM: "xterm" });
    expect(first.status).toBe(0);
    expect(first.stdout).toContain("incodex:status");

    const secondBin = join(context.home, "second-bin");
    mkdirSync(secondBin, { recursive: true });
    writeExecutable(join(secondBin, "incodex"), "#!/bin/sh\nprintf '%s\\n' \"incodex-v2:$*\"\n");
    const second = runGenerated(context, generated.status, [], {
      TERM: "xterm",
      PATH: `${secondBin}:${context.fakeBin}:/usr/bin:/bin`,
    });
    expect(second.status).toBe(0);
    expect(second.stdout).toContain("incodex-v2:status");
  });

  test("the runner finds supported stable CLI paths with a minimal GUI PATH", () => {
    const context = fixture();
    expect(runSetup(context, [], { PATH: "/usr/bin:/bin" }).status).toBe(0);
    const stableBin = join(context.home, ".local", "bin");
    mkdirSync(join(stableBin, "incodex"), { recursive: true });
    writeExecutable(join(stableBin, "inc"), "#!/bin/sh\nprintf '%s\\n' \"stable-home:$*\"\n");

    const generated = paths(context);
    const result = runGenerated(context, generated.status, [], { TERM: "xterm", PATH: "" });
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("stable-home:status");

    const runner = readFileSync(generated.runner, "utf8");
    for (const location of [
      "$HOME/.local/bin/incodex",
      "$HOME/.local/bin/inc",
      "/opt/homebrew/bin/incodex",
      "/opt/homebrew/bin/inc",
      "/usr/local/bin/incodex",
      "/usr/local/bin/inc",
    ]) {
      expect(runner).toContain(location);
    }
  });

  test("Raycast status and doctor use TERM directly or route through a terminal fallback", () => {
    const context = fixture();
    expect(runSetup(context).status).toBe(0);
    const generated = paths(context);
    for (const command of ["status", "doctor"] as const) {
      const source = readFileSync(generated[command], "utf8");
      expect(source).toContain("@raycast.mode fullOutput");
      expect(source).toContain(`# ${generatedMarker}`);
    }

    const direct = runGenerated(context, generated.status, [], { TERM: "xterm-256color" });
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

    const fallback = runGenerated(context, generated.status, [], {
      TERM: "dumb",
      INCODEX_LAUNCHER_APP: "Hyper",
    });
    expect(fallback.status).toBe(0);
    expect(readFileSync(join(context.home, "open-args"), "utf8")).toContain("Hyper");
    expect(readFileSync(join(context.home, "osascript-script"), "utf8")).toContain(
      'tell application "Terminal"',
    );
  });

  test("Raycast and Alfred preserve terminal-specific Hyper, WindTerm, and Warp routing", () => {
    const context = fixture();
    const app = enableAlfredApp(context);
    expect(runSetup(context, [], { INCODEX_ALFRED_APP: app }).status).toBe(0);
    const generated = paths(context);
    writeExecutable(
      join(context.fakeBin, "open"),
      "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/open-args\"\nexit 0\n",
    );

    for (const terminal of ["Hyper", "WindTerm", "Warp"]) {
      mkdirSync(join(context.home, "Applications", `${terminal}.app`), { recursive: true });
      const launched = runGenerated(context, generated.status, [], {
        TERM: "dumb",
        INCODEX_LAUNCHER_APP: terminal,
      });
      expect(launched.status).toBe(0);
      const args = readFileSync(join(context.home, "open-args"), "utf8").split("\n");
      expect(args).toContain(terminal);
      expect(args).toContain("--args");
      expect(args).not.toContain("-e");
    }

    const alfredRunner = unzipEntry(generated.workflow, "run.sh");
    for (const terminal of ["Hyper", "WindTerm", "Warp"]) {
      expect(readFileSync(generated.runner, "utf8")).toContain(terminal);
    }
    expect(alfredRunner).toContain(`# ${generatedMarker}`);
    expect(unzipEntry(generated.workflow, "info.plist")).not.toContain("Alfred.alfredpreferences");

    const extracted = join(context.home, "alfred-run.sh");
    writeExecutable(extracted, alfredRunner);
    const launched = runGenerated(context, extracted, ["status"], { TERM: "xterm" });
    expect(launched.status).toBe(0);
    expect(launched.stdout).toContain("incodex:status");
  });

  test("uses Raycast's documented shared script directory without provider preferences", () => {
    const source = readFileSync(setupScript, "utf8");
    expect(source).toContain("$HOME/Library/Application Support/Raycast/script-commands");
    expect(source).not.toContain("com.raycast.macos.plist");
    expect(source).not.toContain("defaults write");
    expect(source).not.toMatch(/(?:curl|wget).*(?:main|master)/);
  });

  test("keeps setup install-only and free of the retired transaction machinery", () => {
    const source = readFileSync(setupScript, "utf8");
    expect(source).not.toContain("uninstall");
    for (const retired of ["manifest", "PUBLISH_", "rollback", "install.lock", "LOCK_", "token"]) {
      expect(source).not.toContain(retired);
    }
    const context = fixture();
    expect(runSetup(context, ["uninstall"]).status).not.toBe(0);
  });

  test("repeated and concurrent setup leave executable generated outputs", async () => {
    const context = fixture();
    expect(runSetup(context).status).toBe(0);
    const generated = paths(context);
    writeFileSync(generated.runner, `#!/bin/bash\n# ${generatedMarker}\n# stale body\n`);
    expect(runSetup(context).status).toBe(0);
    expect(readFileSync(generated.runner, "utf8")).toContain("resolve_incodex");

    const [first, second] = await Promise.all([runSetupAsync(context), runSetupAsync(context)]);
    expect(first.status).toBe(0);
    expect(second.status).toBe(0);
    const status = runGenerated(context, generated.status, [], { TERM: "xterm" });
    expect(status.status).toBe(0);
    expect(status.stdout).toContain("incodex:status");
  });

  test("a per-target publication failure is healed by rerunning setup", () => {
    const context = fixture();
    writeFailOnceStatusMove(context.fakeBin);
    const failed = runSetup(context);
    expect(failed.status).not.toBe(0);

    const retried = runSetup(context);
    expect(retried.status).toBe(0);
    const generated = paths(context);
    for (const artifact of [generated.runner, generated.open, generated.status, generated.doctor]) {
      expect(existsSync(artifact)).toBe(true);
    }
    expect(runGenerated(context, generated.status, [], { TERM: "xterm" }).stdout).toContain("incodex:status");
  });

  test("foreign fixed-name files are never overwritten", () => {
    const cases = [
      (context: ReturnType<typeof fixture>) => {
        mkdirSync(context.root, { recursive: true });
        writeFileSync(join(context.root, "runner.sh"), "foreign runner\n");
        return join(context.root, "runner.sh");
      },
      (context: ReturnType<typeof fixture>) => {
        mkdirSync(context.raycast, { recursive: true });
        writeFileSync(join(context.raycast, "incodex-open.sh"), "foreign Raycast script\n");
        return join(context.raycast, "incodex-open.sh");
      },
      (context: ReturnType<typeof fixture>) => {
        const workflow = paths(context).workflow;
        mkdirSync(dirname(workflow), { recursive: true });
        writeFileSync(workflow, "foreign Alfred package\n");
        return workflow;
      },
    ];

    for (const [index, prepare] of cases.entries()) {
      const context = fixture();
      const foreign = prepare(context);
      const extraEnv: Record<string, string> = index === 2 ? { INCODEX_ALFRED_APP: enableAlfredApp(context) } : {};
      const installed = runSetup(context, [], extraEnv);
      expect(installed.status).not.toBe(0);
      expect(readFileSync(foreign, "utf8")).toContain("foreign");
      expect(existsSync(paths(context).status)).toBe(false);
    }
  });

  test("the generated marker permits rewriting a self-owned target", () => {
    const context = fixture();
    expect(runSetup(context).status).toBe(0);
    const generated = paths(context);
    writeFileSync(generated.open, `${readFileSync(generated.open, "utf8")}# user edit\n`);
    writeFileSync(generated.runner, `${readFileSync(generated.runner, "utf8")}# user edit\n`);

    expect(runSetup(context).status).toBe(0);
    expect(readFileSync(generated.open, "utf8")).not.toContain("# user edit");
    expect(readFileSync(generated.runner, "utf8")).not.toContain("# user edit");
  });

  test("install refuses a symlink in a launcher root parent", () => {
    const context = fixture();
    const victim = mkdtempSync(join(tmpdir(), "incodex-launchers-parent-victim-"));
    const redirectedParent = join(context.home, "redirected-parent");
    symlinkSync(victim, redirectedParent);

    const installed = runSetup(context, [], {
      INCODEX_QUICK_LAUNCHERS_ROOT: join(redirectedParent, "quick-launchers"),
    });
    expect(installed.status).not.toBe(0);
    expect(existsSync(join(victim, "runner.sh"))).toBe(false);
    expect(installed.stdout + installed.stderr).toMatch(/symlink|redirect/i);
  });

  test("install refuses symlinked provider directories", () => {
    for (const provider of ["raycast", "alfred"] as const) {
      const context = fixture();
      const victim = mkdtempSync(join(tmpdir(), `incodex-launchers-${provider}-victim-`));
      if (provider === "raycast") {
        mkdirSync(dirname(context.raycast), { recursive: true });
        symlinkSync(victim, context.raycast);
      } else {
        mkdirSync(context.root, { recursive: true });
        symlinkSync(victim, join(context.root, "alfred"));
      }

      const extraEnv: Record<string, string> =
        provider === "alfred" ? { INCODEX_ALFRED_APP: enableAlfredApp(context) } : {};
      const installed = runSetup(context, [], extraEnv);
      expect(installed.status).not.toBe(0);
      expect(existsSync(join(victim, "runner.sh"))).toBe(false);
      expect(existsSync(join(victim, "incodex-open.sh"))).toBe(false);
      expect(installed.stdout + installed.stderr).toMatch(/symlink|redirect/i);
    }
  });
});
