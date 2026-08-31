import { describe, expect, test } from "bun:test";
import { createCaptureCdpBridge } from "./cdp-bridge.ts";

describe("capture window CDP debug bridge", () => {
  test("queues one typed capture request and resolves its matching PNG", async () => {
    const bridge = createCaptureCdpBridge(() => "capture-1");
    const pending = bridge.capture();

    expect(bridge.takeRequest()).toEqual({ id: "capture-1", kind: "capture" });
    expect(bridge.takeRequest()).toBeNull();

    bridge.resolve({
      dataUrl: "data:image/png;base64,cG5n",
      id: "capture-1",
      ok: true,
    });
    expect(await pending).toBe("data:image/png;base64,cG5n");
  });

  test("keeps unknown replies isolated and rejects a matched failure", async () => {
    const bridge = createCaptureCdpBridge(() => "capture-2");
    const pending = bridge.capture();
    bridge.takeRequest();

    expect(bridge.resolve({ id: "other", ok: false, error: "wrong target" })).toBe(false);
    expect(bridge.resolve({ id: "capture-2", ok: false, error: "capture failed" })).toBe(true);
    await expect(pending).rejects.toThrow("capture failed");
  });

  test("rejects malformed successful payloads instead of accepting non-PNG data", async () => {
    const bridge = createCaptureCdpBridge(() => "capture-3");
    const pending = bridge.capture();
    bridge.takeRequest();

    bridge.resolve({ dataUrl: "https://example.com/image.png", id: "capture-3", ok: true });
    await expect(pending).rejects.toThrow("invalid PNG capture payload");
  });
});
