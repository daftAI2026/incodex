/**
 * [INPUT]: 依赖Mac普通窗口、renderer generation scope 与token化动作请求。
 * [OUTPUT]: 只重试UNKNOWN_ACTION，并取消排队请求的后续重试。
 * [POS]: 连接新injector与已提交main actions；不参与UI ACK或Keychain操作。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
export type DeviceKeyActivationResponse = { ok?: boolean; code?: string } | null | undefined;

export type DeviceKeyActivationOptions = {
  platform: string | undefined;
  incognito: boolean;
  scope: Pick<Window, "addEventListener" | "removeEventListener" | "setTimeout" | "clearTimeout">;
  request(): Promise<DeviceKeyActivationResponse>;
};

const MAX_ATTEMPTS = 5;
const RETRY_DELAY_MS = 100;

export function createDeviceKeyActivationRetry(options: DeviceKeyActivationOptions): {
  start(): void;
  dispose(): void;
} {
  let started = false;
  let disposed = false;
  let inFlight = false;
  let attempts = 0;
  let timer: number | undefined;

  const pagehide = () => dispose();

  function start(): void {
    if (started || disposed || options.platform !== "darwin" || options.incognito) return;
    started = true;
    options.scope.addEventListener("pagehide", pagehide, { once: true });
    void attempt();
  }

  async function attempt(): Promise<void> {
    if (disposed || inFlight || attempts >= MAX_ATTEMPTS) return;
    attempts += 1;
    inFlight = true;
    try {
      const response = await options.request();
      if (!disposed && response?.code === "UNKNOWN_ACTION" && attempts < MAX_ATTEMPTS) {
        timer = options.scope.setTimeout(() => {
          timer = undefined;
          void attempt();
        }, RETRY_DELAY_MS);
      }
    } catch {
      // IPC failure is terminal. Only the old action generation is retried.
    } finally {
      inFlight = false;
    }
  }

  function dispose(): void {
    if (disposed) return;
    disposed = true;
    if (timer !== undefined) options.scope.clearTimeout(timer);
    timer = undefined;
    options.scope.removeEventListener("pagehide", pagehide);
  }

  return Object.freeze({ start, dispose });
}
