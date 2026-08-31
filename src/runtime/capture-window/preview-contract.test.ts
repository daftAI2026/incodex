import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dir, "../../..");

describe("capture window preview", () => {
  test("is a local browser harness over the reusable editor", () => {
    const packageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as {
      scripts?: Record<string, string>;
    };
    const server = readFileSync(join(root, "scripts/capture-window-preview.ts"), "utf8");
    const preview = readFileSync(join(import.meta.dir, "preview.ts"), "utf8");

    expect(packageJson.scripts?.["preview:capture-window"]).toBe(
      "bun scripts/capture-window-preview.ts",
    );
    expect(server).toContain("127.0.0.1");
    expect(preview).toContain("mountCaptureWindowEditor");
    expect(preview).not.toContain("incodex-main.cts");
    expect(`${server}\n${preview}`).not.toMatch(/https?:\/\//);
  });

  test("keeps Lucide icons local and current-color driven", () => {
    const icons = readFileSync(join(import.meta.dir, "icons.ts"), "utf8");

    expect(icons).toContain('stroke="currentColor"');
    expect(icons).toContain('stroke-linecap="round"');
    expect(icons).not.toContain('src="http');
    expect(icons).not.toContain("fetch(");
  });
});
