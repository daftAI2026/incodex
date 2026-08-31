export type CaptureDebugRequest = {
  id: string;
  kind: "capture";
};

export type CaptureDebugResponse =
  | { dataUrl: string; id: string; ok: true }
  | { error: string; id: string; ok: false };

export type CaptureCdpBridge = {
  capture: () => Promise<string>;
  resolve: (response: CaptureDebugResponse) => boolean;
  takeRequest: () => CaptureDebugRequest | null;
};

type PendingCapture = {
  reject: (error: Error) => void;
  resolve: (dataUrl: string) => void;
};

export function createCaptureCdpBridge(
  createId: () => string = defaultCaptureId,
): CaptureCdpBridge {
  const pending = new Map<string, PendingCapture>();
  const requests: CaptureDebugRequest[] = [];

  function capture(): Promise<string> {
    const id = createId();
    requests.push({ id, kind: "capture" });
    return new Promise((resolve, reject) => {
      pending.set(id, { reject, resolve });
    });
  }

  function takeRequest(): CaptureDebugRequest | null {
    return requests.shift() ?? null;
  }

  function resolve(response: CaptureDebugResponse): boolean {
    const capture = pending.get(response.id);
    if (!capture) return false;
    pending.delete(response.id);
    if (!response.ok) {
      capture.reject(new Error(response.error));
      return true;
    }
    if (!response.dataUrl.startsWith("data:image/png;base64,")) {
      capture.reject(new Error("invalid PNG capture payload"));
      return true;
    }
    capture.resolve(response.dataUrl);
    return true;
  }

  return { capture, resolve, takeRequest };
}

function defaultCaptureId(): string {
  return `capture-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}
