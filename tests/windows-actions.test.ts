import { describe, expect, test } from "bun:test";
import { join } from "node:path";

type Response = { ok: boolean; code: string; requestId?: string; reason?: string };
type Payload = { action: string; requestId: string };
type Controller = {
  prepare(factory: unknown, id: string): unknown;
  commit(prepared: unknown): void;
  request(payload: Payload): Promise<Response>;
  generation(): { protocol: number; id: string } | null;
};
const factory = require("../src/runtime/incodex-main-actions.cts").createMainActions;
function controller(nativeOpen: (payload: Payload) => Promise<Response>): Controller {
  return require(join(import.meta.dir, "../crates/incodex-cli/assets/incodex-windows-actions.cjs"))
    .createWindowsActionController(nativeOpen);
}

describe("Windows shared business action generations", () => {
  test("A to B to A keeps pending A and selects B for new requests exactly once", async () => {
    let finishA!: (result: Response) => void;
    const calls: string[] = [];
    const actions = controller(async payload => {
      calls.push(payload.requestId);
      if (payload.requestId === "incodex-pending-a") return new Promise(resolve => { finishA = resolve; });
      return { ok: true, code: "OK", requestId: payload.requestId };
    });
    actions.commit(actions.prepare(factory, "a".repeat(64)));
    const old = actions.request({ action: "open", requestId: "incodex-pending-a" });
    const b = actions.prepare((deps: unknown) => {
      const shared = factory(deps);
      return { ...shared, handle: async (...args: unknown[]) => {
        const result = await shared.handle(...args);
        return result.ok ? { ...result, code: "OK_B" } : result;
      } };
    }, "b".repeat(64));
    expect(actions.generation()?.id).toBe("a".repeat(64));
    actions.commit(b);
    expect(await actions.request({ action: "open", requestId: "incodex-new-b" }))
      .toMatchObject({ ok: true, code: "OK_B", requestId: "incodex-new-b" });
    finishA({ ok: true, code: "OK" });
    expect(await old).toMatchObject({ ok: true, code: "OK", requestId: "incodex-pending-a" });
    actions.commit(actions.prepare(factory, "a".repeat(64)));
    expect(await actions.request({ action: "open", requestId: "incodex-new-a" }))
      .toMatchObject({ ok: true, code: "OK", requestId: "incodex-new-a" });
    expect(calls).toEqual(["incodex-pending-a", "incodex-new-b", "incodex-new-a"]);
  });

  test("failed factory preparation retains the active generation", async () => {
    const actions = controller(async () => ({ ok: true, code: "OK" }));
    actions.commit(actions.prepare(factory, "a".repeat(64)));
    for (const bad of [() => { throw new Error("failed candidate"); }, () => ({ protocol: 2 }), () => ({ protocol: 1 })]) {
      expect(() => actions.prepare(bad, "b".repeat(64))).toThrow();
      expect(actions.generation()?.id).toBe("a".repeat(64));
    }
    expect(await actions.request({ action: "open", requestId: "incodex-retained-a" })).toMatchObject({ code: "OK" });
  });

  test("business factory cannot widen the installed native bridge beyond open", async () => {
    let calls = 0;
    const actions = controller(async () => { calls++; return { ok: false, code: "CDP_UNAVAILABLE" }; });
    actions.commit(actions.prepare(factory, "a".repeat(64)));
    for (const action of ["quit", "configure-dock-menu", "configure-status-menu", "unknown"]) {
      expect(await actions.request({ action, requestId: "incodex-forbidden" })).toMatchObject({ ok: false, code: "UNKNOWN_ACTION" });
    }
    expect(calls).toBe(0);
    expect(await actions.request({ action: "open", requestId: "incodex-error" }))
      .toMatchObject({ ok: false, code: "CDP_UNAVAILABLE", requestId: "incodex-error" });
    expect(calls).toBe(1);
  });
});
