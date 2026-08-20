import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { describe, expect, test } from "bun:test";
import { cliVersion } from "../src/cli-version";
import { CONFIRM_PROMPT } from "../src/confirm-prompt";
import { requireYesMessage } from "../src/confirm";
import { commandHelp, rootHelp } from "../src/help";
import { MENU_BANNER, MENU_ITEMS, MENU_REPO_URL, MENU_TAGLINE } from "../src/menu";
import type { CliCommand } from "../src/parse-cli";

const root = join(import.meta.dir, "..");
const cli = join(root, "src/cli.ts");

const HELP_COMMANDS: CliCommand[] = [
  "install",
  "uninstall",
  "status",
  "doctor",
  "runtime",
  "recover",
  "open",
  "update",
  "self-uninstall",
];

const DIAGNOSIS_KEYS = [
  "target",
  "targetId",
  "exists",
  "patched",
  "bundleId",
  "appVersion",
  "appBuild",
  "architecture",
  "asarFileHash",
  "asarHeaderHash",
  "plistFileHash",
  "plistIntegrityHash",
  "runtimeVersion",
  "originalMain",
  "codesignOk",
  "backup",
  "stalePid",
  "orphanSessions",
  "leftoverChromium",
  "asarLoaderOnly",
  "externalRuntime",
  "signing",
  "spctl",
  "interruptedTransactions",
] as const;

const EXTERNAL_RUNTIME_KEYS = ["present", "ok", "version", "release", "error"] as const;
const TRANSACTION_KEYS = ["installId", "phase", "action"] as const;

type CliResult = {
  status: number;
  stdout: string;
  stderr: string;
};

function isolatedHome(): string {
  return mkdtempSync(join(tmpdir(), "incodex-golden-"));
}

function runCli(args: string[], home: string, extraEnv: NodeJS.Dict<string> = {}): CliResult {
  const ran = spawnSync("bun", [cli, ...args], {
    cwd: root,
    encoding: "utf8",
    env: {
      ...process.env,
      HOME: home,
      TERM: "dumb",
      NO_COLOR: "1",
      SHELL: extraEnv.SHELL ?? "/bin/zsh",
      ...extraEnv,
    },
  });
  return {
    status: ran.status ?? 1,
    stdout: ran.stdout ?? "",
    stderr: ran.stderr ?? "",
  };
}

function incodexPaths(home: string): string[] {
  const dir = join(home, ".incodex");
  if (!existsSync(dir)) return [];
  const out: string[] = [];
  const walk = (current: string) => {
    for (const name of readdirSync(current).sort()) {
      const full = join(current, name);
      out.push(relative(dir, full));
      if (statSync(full).isDirectory()) walk(full);
    }
  };
  walk(dir);
  return out;
}

function markerApp(home: string): string {
  const app = join(home, "Marker.app");
  mkdirSync(app, { recursive: true });
  writeFileSync(join(app, "marker"), "do-not-touch\n");
  return app;
}

function parseJson(stdout: string): unknown {
  return JSON.parse(stdout);
}

function keysOf(value: object): string[] {
  return Object.keys(value);
}

function visible(text: string): string {
  const ansi = new RegExp(`${String.fromCharCode(27)}\\[[0-9;]*[A-Za-z]`, "g");
  return text.replace(ansi, "").replace(/\r/g, "");
}

function installMutations(home: string): string[] {
  return incodexPaths(home).filter((path) => !path.startsWith("cache"));
}

function runTty(args: string[], home: string, waitFor: string, keys: string): CliResult {
  const script = `
import os, pty, select, sys, time
home, wait_for, keys = sys.argv[1], sys.argv[2].encode("utf-8"), sys.argv[3].encode("latin-1")
cli_args = sys.argv[4:]
env = os.environ.copy()
env["HOME"] = home
env["TERM"] = "xterm-256color"
env["SHELL"] = env.get("SHELL") or "/bin/zsh"
pid, fd = pty.fork()
if pid == 0:
    os.chdir(${JSON.stringify(root)})
    os.execvpe("bun", ["bun", *cli_args], env)
buf = bytearray()
sent = False
deadline = time.time() + 8
while time.time() < deadline:
    ready, _, _ = select.select([fd], [], [], 0.1)
    if not ready:
        if sent:
            break
        continue
    try:
        chunk = os.read(fd, 8192)
    except OSError:
        break
    if not chunk:
        break
    buf.extend(chunk)
    if not sent and wait_for in buf:
        os.write(fd, keys)
        sent = True
try:
    _, status = os.waitpid(pid, 0)
except ChildProcessError:
    status = 0
code = os.waitstatus_to_exitcode(status) if hasattr(os, "waitstatus_to_exitcode") else (os.WEXITSTATUS(status) if os.WIFEXITED(status) else 1)
sys.stdout.buffer.write(b"STATUS %d\\n" % code)
sys.stdout.buffer.write(bytes(buf))
`;
  const ran = spawnSync("python3", ["-c", script, home, waitFor, keys, cli, ...args], {
    cwd: root,
    encoding: "utf8",
    env: { ...process.env, HOME: home, TERM: "xterm-256color" },
  });
  const raw = ran.stdout ?? "";
  const nl = raw.indexOf("\n");
  const statusLine = nl === -1 ? raw : raw.slice(0, nl);
  const body = nl === -1 ? "" : raw.slice(nl + 1);
  const status = Number(statusLine.replace("STATUS ", "").trim());
  return {
    status: Number.isFinite(status) ? status : ran.status ?? 1,
    stdout: body,
    stderr: ran.stderr ?? "",
  };
}

describe("golden CLI: help and version", () => {
  test("non-TTY no-args prints root help and exits 0", () => {
    const home = isolatedHome();
    const ran = runCli([], home);
    expect(ran.status).toBe(0);
    expect(ran.stderr).toBe("");
    expect(ran.stdout).toBe(`${rootHelp()}\n`);
  });

  test("--help, -h, and help match the same root help", () => {
    const home = isolatedHome();
    const expected = `${rootHelp()}\n`;
    for (const args of [["--help"], ["-h"], ["help"]]) {
      const ran = runCli(args, home);
      expect(ran.status).toBe(0);
      expect(ran.stderr).toBe("");
      expect(ran.stdout).toBe(expected);
    }
  });

  test("each command --help and -h match commandHelp", () => {
    const home = isolatedHome();
    for (const command of HELP_COMMANDS) {
      const expected = `${commandHelp(command)}\n`;
      for (const flag of ["--help", "-h"]) {
        const ran = runCli([command, flag], home);
        expect(ran.status).toBe(0);
        expect(ran.stderr).toBe("");
        expect(ran.stdout).toBe(expected);
      }
    }
  });

  test("--version, -V, and version print the same report shape", () => {
    const home = isolatedHome();
    const reports = [["--version"], ["-V"], ["version"]].map((args) => runCli(args, home));
    for (const ran of reports) {
      expect(ran.status).toBe(0);
      expect(ran.stderr).toBe("");
      const lines = ran.stdout.split("\n");
      expect(lines[0]).toBe(`Incodex version ${cliVersion()}`);
      expect(lines[1]).toMatch(/^macOS: /);
      expect(lines[2]).toMatch(/^Architecture: /);
      expect(lines[3]).toMatch(/^Kernel: /);
      expect(lines[4]).toMatch(/^SIP: (Enabled|Disabled|Unknown)$/);
      expect(lines[5]).toMatch(/^Disk Free: (\d+\.\d{2}GB|Unknown)$/);
      expect(lines[6]).toBe("Install: Source");
      expect(lines[7]).toBe("Shell: /bin/zsh");
      expect(lines[8]).toBe("");
      expect(lines[9]).toBe("");
      expect(ran.stdout.endsWith("\n\n")).toBe(true);
    }
    const stableReport = (stdout: string) =>
      stdout.replace(/^Disk Free: .*$/m, "Disk Free: <live value>");
    expect(stableReport(reports[1]?.stdout ?? "")).toBe(stableReport(reports[0]?.stdout ?? ""));
    expect(stableReport(reports[2]?.stdout ?? "")).toBe(stableReport(reports[0]?.stdout ?? ""));
  });

  test("version wins over --help; --version is not a flag on other commands", () => {
    const home = isolatedHome();
    const versionHelp = runCli(["version", "--help"], home);
    expect(versionHelp.status).toBe(0);
    expect(versionHelp.stdout.startsWith(`Incodex version ${cliVersion()}\n`)).toBe(true);

    const helpVersion = runCli(["help", "--version"], home);
    expect(helpVersion.status).toBe(1);
    expect(helpVersion.stdout).toBe("");
    expect(helpVersion.stderr).toBe("unknown flag: --version\n  incodex --help\n");
  });
});

describe("golden CLI: TTY menu vs non-TTY help", () => {
  test("TTY no-args draws the menu and q quits without mutating ~/.incodex", () => {
    const home = isolatedHome();
    const ran = runTty([], home, "Quit", "q");
    expect(ran.status).toBe(0);
    expect(ran.stdout).toContain(MENU_BANNER.split("\n")[0]!);
    expect(ran.stdout).toContain(MENU_REPO_URL);
    expect(ran.stdout).toContain(MENU_TAGLINE);
    for (const item of MENU_ITEMS) {
      expect(ran.stdout).toContain(item.title);
      expect(ran.stdout).toContain(item.description);
    }
    expect(visible(ran.stdout)).toContain("↑↓ | Enter | V Version | Q Quit | 1-6 Jump");
    expect(installMutations(home)).toEqual([]);
  }, 15_000);
});

describe("golden CLI: JSON schema", () => {
  test("status --json and doctor --json share one Diagnosis object", () => {
    const home = isolatedHome();
    const app = join(home, "Missing.app");
    const status = runCli(["status", "--json", "--app", app], home);
    const doctor = runCli(["doctor", "--json", "--app", app], home);
    expect(status.status).toBe(0);
    expect(doctor.status).toBe(0);
    expect(status.stderr).toBe("");
    expect(doctor.stderr).toBe("");
    expect(status.stdout).toBe(doctor.stdout);

    const body = parseJson(status.stdout);
    expect(body && typeof body === "object").toBe(true);
    const rec = body as Record<string, unknown>;
    expect(keysOf(rec)).toEqual([...DIAGNOSIS_KEYS]);
    expect(rec.target).toBe(app);
    expect(rec.targetId).toMatch(/^app-[0-9a-f]{12}$/);
    expect(rec.exists).toBe(false);
    expect(rec.patched).toBe(false);
    expect(rec.bundleId).toBeNull();
    expect(rec.appVersion).toBeNull();
    expect(rec.appBuild).toBeNull();
    expect(rec.architecture).toBeNull();
    expect(rec.asarFileHash).toBeNull();
    expect(rec.asarHeaderHash).toBeNull();
    expect(rec.plistFileHash).toBeNull();
    expect(rec.plistIntegrityHash).toBeNull();
    expect(rec.runtimeVersion).toBeNull();
    expect(rec.originalMain).toBe("");
    expect(rec.codesignOk).toBe(false);
    expect(rec.backup).toBeNull();
    expect(rec.stalePid).toBe(false);
    expect(rec.orphanSessions).toEqual([]);
    expect(rec.leftoverChromium).toEqual([]);
    expect(rec.asarLoaderOnly).toBeNull();
    expect(rec.signing).toBeNull();
    expect(rec.spctl).toBeNull();
    expect(rec.interruptedTransactions).toEqual([]);

    const runtime = rec.externalRuntime as Record<string, unknown>;
    expect(keysOf(runtime)).toEqual([...EXTERNAL_RUNTIME_KEYS]);
    expect(runtime.present).toBe(false);
    expect(runtime.ok).toBe(false);
    expect(runtime.version).toBeNull();
    expect(runtime.release).toBeNull();
    expect(runtime.error).toBe("missing current.json");
  });

  test("doctor --json names interrupted journals with rollback action", () => {
    const home = isolatedHome();
    const app = join(home, "Missing.app");
    const txDir = join(home, ".incodex", "transactions");
    mkdirSync(txDir, { recursive: true });
    writeFileSync(
      join(txDir, "tx-golden.json"),
      `${JSON.stringify(
        {
          schemaVersion: 1,
          installId: "tx-golden",
          targetRealPath: app,
          stagedApp: join(home, "staged"),
          originalSnapshot: join(home, "original"),
          phase: "PATCHED",
          updatedAt: "2026-01-01T00:00:00.000Z",
        },
        null,
        2,
      )}\n`,
    );
    const ran = runCli(["doctor", "--json", "--app", app], home);
    expect(ran.status).toBe(0);
    const rec = parseJson(ran.stdout) as Record<string, unknown>;
    expect(keysOf(rec)).toEqual([...DIAGNOSIS_KEYS]);
    const txs = rec.interruptedTransactions as Record<string, unknown>[];
    expect(txs).toHaveLength(1);
    expect(keysOf(txs[0]!)).toEqual([...TRANSACTION_KEYS]);
    expect(txs[0]).toEqual({ installId: "tx-golden", phase: "PATCHED", action: "rollback" });
  });
});

describe("golden CLI: read-only human output", () => {
  test("status for a missing --app warns and does not create ~/.incodex", () => {
    const home = isolatedHome();
    const app = join(home, "Missing.app");
    const ran = runCli(["status", "--app", app], home);
    expect(ran.status).toBe(0);
    expect(ran.stderr).toBe("");
    expect(ran.stdout).toBe(`➤ Status\n  ! Codex app not found: ${app}\n\n`);
    expect(incodexPaths(home)).toEqual([]);
  });

  test("doctor for a missing --app prints labeled sections", () => {
    const home = isolatedHome();
    const app = join(home, "Missing.app");
    const ran = runCli(["doctor", "--app", app], home);
    expect(ran.status).toBe(0);
    expect(ran.stderr).toBe("");
    expect(ran.stdout).toBe(
      [
        "➤ App",
        `  Path         ${app}`,
        "  Exists       no",
        "  Installed    no",
        "  Bundle       unknown",
        "  Version      unknown",
        "  Arch         unknown",
        "",
        "➤ Runtime",
        "  Version      unknown",
        "  External     missing",
        "  ! missing current.json",
        "  Loader       unknown",
        "  Main         unknown",
        "",
        "➤ Signing",
        "  Verify       failed",
        "",
        "➤ Backup",
        "  State        none",
        "",
        "➤ Sessions",
        "  Orphans      0",
        "  Chromium     0",
        "  Stale pid    no",
        "  Journals     0",
        "",
        "",
      ].join("\n"),
    );
  });
});

describe("golden CLI: --dry-run does not mutate the filesystem", () => {
  test("install --dry-run --app leaves the marker and does not write ~/.incodex", () => {
    const home = isolatedHome();
    const app = markerApp(home);
    const ran = runCli(["install", "--dry-run", "--app", app], home);
    expect(ran.status).toBe(0);
    expect(ran.stderr).toBe("");
    expect(ran.stdout).toContain("➤ Install");
    expect(ran.stdout).toContain(app);
    expect(ran.stdout).toContain("  ! Dry run. No files changed.");
    expect(readFileSync(join(app, "marker"), "utf8")).toBe("do-not-touch\n");
    expect(incodexPaths(home)).toEqual([]);
  });

  test("install -n is the same as --dry-run", () => {
    const home = isolatedHome();
    const app = markerApp(home);
    const dashed = runCli(["install", "--dry-run", "--app", app], home);
    const short = runCli(["install", "-n", "--app", app], home);
    expect(short.status).toBe(0);
    expect(short.stdout).toBe(dashed.stdout);
    expect(incodexPaths(home)).toEqual([]);
  });

  test("uninstall --dry-run --app does not change the marker", () => {
    const home = isolatedHome();
    const app = markerApp(home);
    const ran = runCli(["uninstall", "--dry-run", "--app", app], home);
    expect(ran.status).toBe(0);
    expect(ran.stderr).toBe("");
    expect(ran.stdout).toBe(`➤ Uninstall\n  App          ${app}\n  ! Dry run. No files changed.\n`);
    expect(readFileSync(join(app, "marker"), "utf8")).toBe("do-not-touch\n");
    expect(incodexPaths(home)).toEqual([]);
  });

  test("install --clone --dry-run does not create the scratch copy", () => {
    const home = isolatedHome();
    const ran = runCli(["install", "--clone", "--dry-run"], home);
    expect(ran.status).toBe(0);
    expect(ran.stderr).toBe("");
    expect(ran.stdout).toContain("➤ Clone install");
    expect(ran.stdout).toContain("  ! Dry run. No files changed.");
    expect(existsSync(join(home, ".incodex", "scratch", "ChatGPT.app"))).toBe(false);
    expect(incodexPaths(home)).toEqual([]);
  });

  test("open --dry-run does not create a session", () => {
    const home = isolatedHome();
    const app = markerApp(home);
    const ran = runCli(["open", "--dry-run", "--app", app], home);
    expect(ran.status).toBe(0);
    expect(ran.stderr).toBe("");
    expect(ran.stdout).toContain("➤ Open incognito without patching Codex");
    expect(ran.stdout).toContain(`  App          ${app}`);
    expect(ran.stdout).toContain(`  Binary       ${join(app, "Contents/MacOS/ChatGPT")}`);
    expect(ran.stdout).toContain("  ! Dry run. No window opened.");
    expect(incodexPaths(home)).toEqual([]);
  });

  test("runtime --dry-run does not write ~/.incodex/runtime", () => {
    const home = isolatedHome();
    const ran = runCli(["runtime", "--dry-run"], home);
    expect(ran.status).toBe(0);
    expect(ran.stderr).toBe("");
    expect(ran.stdout).toBe("would update ~/.incodex/runtime/ without modifying Codex\n");
    expect(incodexPaths(home)).toEqual([]);
  });

  test("official install --dry-run does not write ~/.incodex", () => {
    const home = isolatedHome();
    const ran = runCli(["install", "--dry-run"], home);
    expect(ran.status).toBe(0);
    expect(ran.stderr).toBe("");
    expect(ran.stdout).toContain("➤ Install");
    expect(ran.stdout).toContain("/Applications/ChatGPT.app");
    expect(ran.stdout).toContain("  ! Dry run. No files changed.");
    expect(incodexPaths(home)).toEqual([]);
  });
});

describe("golden CLI: confirmation", () => {
  test("non-TTY official install and uninstall require --yes and do not mutate ~/.incodex", () => {
    const home = isolatedHome();
    const install = runCli(["install"], home);
    expect(install.status).toBe(1);
    expect(install.stderr).toBe(`${requireYesMessage("install")}\n`);
    expect(install.stdout).toContain("➤ Install");
    expect(install.stdout).toContain("/Applications/ChatGPT.app");
    expect(incodexPaths(home)).toEqual([]);

    const uninstall = runCli(["uninstall"], home);
    expect(uninstall.status).toBe(1);
    expect(uninstall.stderr).toBe(`${requireYesMessage("uninstall")}\n`);
    expect(uninstall.stdout).toBe("➤ Uninstall\n  App          /Applications/ChatGPT.app\n");
    expect(incodexPaths(home)).toEqual([]);
  });

  test("non-TTY --app install requires --yes and leaves the marker", () => {
    const home = isolatedHome();
    const app = markerApp(home);
    const ran = runCli(["install", "--app", app], home);
    expect(ran.status).toBe(1);
    expect(ran.stderr).toBe(`${requireYesMessage("install")}\n`);
    expect(readFileSync(join(app, "marker"), "utf8")).toBe("do-not-touch\n");
    expect(incodexPaths(home)).toEqual([]);
  });

  test("TTY install --app asks once; ESC aborts without writing ~/.incodex", () => {
    const home = isolatedHome();
    const app = markerApp(home);
    const ran = runTty(["install", "--app", app], home, CONFIRM_PROMPT, "\u001b");
    expect(ran.status).toBe(1);
    expect(ran.stdout).toContain("\u001b[1;35m");
    expect(visible(ran.stdout)).toContain("➤ Install");
    expect(visible(ran.stdout)).toContain(CONFIRM_PROMPT);
    expect(visible(ran.stdout)).toContain("aborted");
    expect(readFileSync(join(app, "marker"), "utf8")).toBe("do-not-touch\n");
    expect(installMutations(home)).toEqual([]);
  }, 15_000);

  test("clone and --dry-run skip --yes in non-TTY", () => {
    const home = isolatedHome();
    const clone = runCli(["install", "--clone", "--dry-run"], home);
    expect(clone.status).toBe(0);
    expect(clone.stderr).toBe("");
    const dry = runCli(["install", "--dry-run", "--app", markerApp(home)], home);
    expect(dry.status).toBe(0);
    expect(dry.stderr).toBe("");
  });

  test("--confirm-live is accepted as --yes on a dry-run", () => {
    const home = isolatedHome();
    const app = markerApp(home);
    const ran = runCli(["install", "--confirm-live", "--dry-run", "--app", app], home);
    expect(ran.status).toBe(0);
    expect(ran.stderr).toBe("");
    expect(ran.stdout).toContain("  ! Dry run. No files changed.");
  });
});

describe("golden CLI: fail-closed parse errors", () => {
  test("unknown command, flag, and unexpected argument exit 1 with empty stdout", () => {
    const home = isolatedHome();
    const cases: Array<{ args: string[]; stderr: string }> = [
      { args: ["wipe"], stderr: "unknown command: wipe\n  incodex --help\n" },
      { args: ["menu"], stderr: "unknown command: menu\n  incodex --help\n" },
      { args: ["install", "--please"], stderr: "unknown flag: --please\n  incodex --help\n" },
      { args: ["install", "--dry-run\uFF0C"], stderr: "unknown flag: --dry-run，\n  incodex --help\n" },
      { args: ["install", "--dry-run,"], stderr: "unknown flag: --dry-run,\n  incodex --help\n" },
      { args: ["install", "now"], stderr: "unexpected argument: now\n  incodex --help\n" },
      { args: ["install", "--clone", "--live"], stderr: "--clone and --live cannot be used together\n" },
      { args: ["install", "--clone", "--app", "/tmp/x.app"], stderr: "--clone and --app cannot be used together\n" },
      { args: ["status", "--app"], stderr: "--app requires a path, not another flag\n" },
      { args: ["status", "--app", "--json"], stderr: "--app requires a path, not another flag\n" },
      { args: ["recover"], stderr: "recover requires --transaction <id>\n  incodex recover --transaction <id>\n" },
      { args: ["recover", "--transaction", "does-not-exist"], stderr: "no journal for does-not-exist\n" },
    ];
    for (const item of cases) {
      const ran = runCli(item.args, home);
      expect(ran.status).toBe(1);
      expect(ran.stdout).toBe("");
      expect(ran.stderr).toBe(item.stderr);
      expect(incodexPaths(home)).toEqual([]);
    }
  });

  test("source checkout refuses update and self-uninstall even with --dry-run", () => {
    const home = isolatedHome();
    const update = runCli(["update", "--dry-run"], home);
    expect(update.status).toBe(1);
    expect(update.stdout).toBe("");
    expect(update.stderr).toBe(
      "this copy is running from source\n  git pull && bun install --frozen-lockfile && bun link\n",
    );
    const self = runCli(["self-uninstall", "--dry-run"], home);
    expect(self.status).toBe(1);
    expect(self.stdout).toBe("");
    expect(self.stderr).toBe("this copy is running from source\n  bun unlink\n");
  });
});

describe("golden CLI: unused flags are ignored, not rejected", () => {
  test("status --dry-run still prints status", () => {
    const home = isolatedHome();
    const app = join(home, "Missing.app");
    const ran = runCli(["status", "--dry-run", "--app", app], home);
    expect(ran.status).toBe(0);
    expect(ran.stdout).toContain("➤ Status");
    expect(incodexPaths(home)).toEqual([]);
  });
});

test("golden tests never spawn a live official install", () => {
  const src = readFileSync(import.meta.path, "utf8");
  expect(src).not.toMatch(/runCli\(\["install", "--yes"/);
  expect(src).not.toMatch(/runTty\(\["install"\]/);
});
