import { describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { translate } from "./runtime/incognito-copy";
import {
  EXIT_PATHS,
  absolutePrivacyClaimAllowed,
  createForensicSession,
  handleExit,
  plantPrompt,
  scanForPrompt,
  sessionEvidenceGone,
  uniquePrompt,
  type ScanRoots,
} from "./forensics";

describe("privacy forensics", () => {
  test.each([...EXIT_PATHS])("%s does not leave the unique prompt in managed session dirs", (kind) => {
    const world = sandbox();
    const session = createForensicSession(world.userRoot, world.sourceHome, kind === "janitor" ? 999999 : 0);
    const prompt = uniquePrompt();
    plantPrompt(session, world.scan, prompt);
    writeFileSync(join(world.sourceHome, "auth.json"), "{\"token\":\"keep\"}\n");

    if (kind === "janitor") {
      handleExit("sigkill", session, world.userRoot);
      expect(sessionEvidenceGone(session, prompt)).toBe(false);
      handleExit("janitor", session, world.userRoot);
    } else if (kind === "sigkill" || kind === "power-off") {
      handleExit(kind, session, world.userRoot);
      expect(sessionEvidenceGone(session, prompt)).toBe(false);
      handleExit("janitor", session, world.userRoot);
    } else {
      handleExit(kind, session, world.userRoot);
    }

    expect(sessionEvidenceGone(session, prompt)).toBe(true);
    const hits = scanForPrompt(
      [
        world.userRoot,
        world.sourceHome,
        world.scan.tmp,
        world.scan.crashDumps,
        world.scan.applicationSupport,
        world.scan.caches,
        world.scan.savedState,
      ],
      prompt,
    );
    expect(hits).toEqual([]);
  });

  test("the scanner finds a prompt left in tmp, crash dumps, or Application Support", () => {
    const world = sandbox();
    const session = createForensicSession(world.userRoot, world.sourceHome);
    const prompt = uniquePrompt();
    plantPrompt(session, world.scan, prompt, true);
    const hits = scanForPrompt(
      [world.scan.tmp, world.scan.crashDumps, world.scan.applicationSupport, world.scan.caches, world.scan.savedState],
      prompt,
    );
    expect(hits.length).toBeGreaterThan(0);
  });

  test("the everyday source CODEX_HOME never receives the unique prompt", () => {
    const world = sandbox();
    const session = createForensicSession(world.userRoot, world.sourceHome);
    const prompt = uniquePrompt();
    plantPrompt(session, world.scan, prompt);
    handleExit("close", session, world.userRoot);
    expect(scanForPrompt([world.sourceHome], prompt)).toEqual([]);
  });

  test("logging the prompt would fail forensics, so production logs must not contain it", () => {
    const world = sandbox();
    const prompt = uniquePrompt();
    writeFileSync(join(world.userRoot, "logs", "incognito.log"), `launch ${prompt}\n`);
    expect(scanForPrompt([join(world.userRoot, "logs")], prompt).length).toBeGreaterThan(0);
  });

  test("absolute no-trace copy stays off until a real-device Electron scan exists", () => {
    expect(absolutePrivacyClaimAllowed()).toBe(false);
    expect(translate("en", "body")).toContain("Temporary session data is cleaned up after a normal exit");
    expect(translate("en", "body")).not.toContain("leaves no record on this Mac");
    expect(translate("zh-CN", "body")).toContain("正常退出后清理");
    expect(translate("zh-CN", "body")).not.toContain("本机完全不留记录");
  });
});

function sandbox(): { userRoot: string; sourceHome: string; scan: ScanRoots } {
  const root = mkdtempSync(join(tmpdir(), "incodex-forensics-"));
  const userRoot = join(root, ".incodex");
  const sourceHome = join(root, ".codex");
  mkdirSync(sourceHome, { recursive: true });
  mkdirSync(join(userRoot, "logs"), { recursive: true });
  const scan: ScanRoots = {
    userRoot,
    sourceHome,
    tmp: join(root, "tmp"),
    crashDumps: join(root, "Library/Logs/DiagnosticReports"),
    applicationSupport: join(root, "Library/Application Support"),
    caches: join(root, "Library/Caches"),
    savedState: join(root, "Library/Saved Application State"),
  };
  for (const dir of [scan.tmp, scan.crashDumps, scan.applicationSupport, scan.caches, scan.savedState]) {
    mkdirSync(dir, { recursive: true });
  }
  return { userRoot, sourceHome, scan };
}
