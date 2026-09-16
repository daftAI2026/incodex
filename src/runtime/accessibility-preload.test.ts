import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";

// Exercise the shipped preload in an isolated permission renderer.
test("permission guide uses native drag IPC and never exposes the general app bridge", () => {
  const listeners: Record<string, (...args: any[]) => void> = {};
  const sent: any[] = [];
  const exposed: any[] = [];
  const nodes = new Map<string, any>();
  for (const id of ["app-icon", "repair", "later", "settings"]) {
    nodes.set(id, { addEventListener: (name: string, fn: any) => { listeners[`${id}:${name}`] = fn; } });
  }
  const window = {}; Object.assign(window, { top: window });
  runInNewContext(readFileSync(new URL("../../dist/incodex-preload.cjs", import.meta.url), "utf8"), {
    require: () => ({
      contextBridge: { exposeInMainWorld: (...args: any[]) => exposed.push(args) },
      ipcRenderer: { send: (...args: any[]) => sent.push(args), on: () => {} },
    }),
    process: { argv: ["--incodex-accessibility-setup"] },
    window, module: { exports: {} },
    document: { addEventListener: (_name: string, fn: any) => fn(), getElementById: (id: string) => nodes.get(id) },
  });
  expect(exposed).toEqual([]);
  let prevented = false;
  listeners["app-icon:dragstart"]?.({ preventDefault: () => { prevented = true; } });
  expect(prevented).toBe(true);
  expect(sent).toEqual([["incodex-accessibility-action", "drag"]]);
});
