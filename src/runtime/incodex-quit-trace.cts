/**
 * [INPUT]: 依赖注入的 Module、绝对 sky.node 路径、有限码日志与可选 Electron app。
 * [OUTPUT]: 提供官方隐私退出 getter 和同步/派发后事件的只读观察句柄。
 * [POS]: 主进程 opt-in 诊断；原位包装官方函数，不重建 sender 判定或改变退出流程。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { isAbsolute } from "node:path";

type LifecycleCode = "app-before-quit" | "app-will-quit" | "app-activate";
type TraceCode =
  | "getter-value" | "getter-nonboolean" | "getter-threw" | "getter-unavailable"
  | "load-threw" | "load-hook-unavailable" | "lifecycle-unavailable"
  | `${LifecycleCode}-sync` | `${LifecycleCode}-after-sync-dispatch`;
type QuitTraceOptions = {
  moduleApi: any;
  skyPath: string;
  record?: (code: TraceCode, value: boolean) => void;
  defer?: (task: () => void) => void;
};

const GETTER = "isPrivacySettingsTerminationRequest";
const EVENTS = [
  ["before-quit", "app-before-quit"],
  ["will-quit", "app-will-quit"],
  ["activate", "app-activate"],
] as const;

function safelyRecord(record: QuitTraceOptions["record"], code: TraceCode, value: boolean): void {
  try { record?.(code, value); } catch {}
}

function isObject(value: unknown): value is Record<string, unknown> {
  return (typeof value === "object" && value !== null) || typeof value === "function";
}

export function createQuitTrace(options: QuitTraceOptions) {
  const { moduleApi, skyPath, record } = options;
  const defer = options.defer ?? ((task: () => void) => queueMicrotask(task));
  let observedExports: object | null = null;
  let originalGetterDescriptor: PropertyDescriptor | null = null;
  let wrappedGetter: Function | null = null;
  let originalLoad: Function | null = null;
  let loadWrapper: Function | null = null;
  let active = true;
  let lifecycleAttached = false;
  const listeners: Array<[any, string, (...args: any[]) => void]> = [];

  function restoreLoadHook(): void {
    if (!moduleApi || !loadWrapper || !originalLoad) return;
    try { if (moduleApi._load === loadWrapper) moduleApi._load = originalLoad; } catch {}
  }

  function restoreGetter(): void {
    if (!observedExports || !wrappedGetter || !originalGetterDescriptor) return;
    try {
      if (Object.getOwnPropertyDescriptor(observedExports, GETTER)?.value === wrappedGetter) {
        Object.defineProperty(observedExports, GETTER, originalGetterDescriptor);
      }
    } catch {}
  }

  function observeExports(exportsValue: unknown): void {
    if (!active) return;
    if (!isObject(exportsValue)) return safelyRecord(record, "getter-unavailable", false);
    let descriptor: PropertyDescriptor | undefined;
    try { descriptor = Object.getOwnPropertyDescriptor(exportsValue, GETTER); } catch {}
    if (!descriptor || !("value" in descriptor) || typeof descriptor.value !== "function" || descriptor.writable !== true) {
      return safelyRecord(record, "getter-unavailable", false);
    }
    const nativeGetter = descriptor.value as Function;
    const wrapper = function (this: unknown, ...args: unknown[]) {
      let result: unknown;
      try { result = Reflect.apply(nativeGetter, this, args); }
      catch (error) { safelyRecord(record, "getter-threw", false); throw error; }
      const booleanResult = typeof result === "boolean" ? result : null;
      safelyRecord(record, booleanResult === null ? "getter-nonboolean" : "getter-value", booleanResult ?? false);
      return result;
    };
    try { Object.defineProperty(exportsValue, GETTER, { ...descriptor, value: wrapper }); }
    catch { return safelyRecord(record, "getter-unavailable", false); }
    observedExports = exportsValue;
    originalGetterDescriptor = descriptor;
    wrappedGetter = wrapper;
  }

  function retireLoadHook(): void {
    restoreLoadHook();
    loadWrapper = null;
    originalLoad = null;
  }

  function installLoadHook(): void {
    if (!moduleApi || !isAbsolute(skyPath)) return safelyRecord(record, "load-hook-unavailable", false);
    let cached: any;
    try { cached = moduleApi._cache?.[skyPath]; } catch {}
    if (cached && isObject(cached) && Object.prototype.hasOwnProperty.call(cached, "exports")) {
      return observeExports(cached.exports);
    }
    let descriptor: PropertyDescriptor | undefined;
    try { descriptor = Object.getOwnPropertyDescriptor(moduleApi, "_load"); } catch {}
    if (!descriptor || !("value" in descriptor) || descriptor.writable !== true || typeof descriptor.value !== "function") {
      return safelyRecord(record, "load-hook-unavailable", false);
    }
    const savedLoad = descriptor.value as Function;
    originalLoad = savedLoad;
    const wrapper = function (this: unknown, ...args: unknown[]) {
      if (args[0] !== skyPath) return Reflect.apply(savedLoad, this, args);
      try {
        const exportsValue = Reflect.apply(savedLoad, this, args);
        try { observeExports(exportsValue); } catch { safelyRecord(record, "getter-unavailable", false); }
        return exportsValue;
      } catch (error) { safelyRecord(record, "load-threw", false); throw error; }
      finally { retireLoadHook(); }
    };
    loadWrapper = wrapper;
    try {
      moduleApi._load = wrapper;
      if (moduleApi._load !== wrapper) throw new Error("loader hook unavailable");
    } catch {
      loadWrapper = null;
      originalLoad = null;
      safelyRecord(record, "load-hook-unavailable", false);
    }
  }

  installLoadHook();

  function defaultPrevented(event: unknown): boolean {
    try { return isObject(event) && (event as any).defaultPrevented === true; } catch { return false; }
  }

  function attachLifecycle(app: any): void {
    if (lifecycleAttached || !active) return;
    lifecycleAttached = true;
    try {
      if (!app || typeof app.on !== "function") throw new Error("app listener unavailable");
    } catch { return safelyRecord(record, "lifecycle-unavailable", false); }
    for (const [eventName, baseCode] of EVENTS) {
      const listener = (event: unknown) => {
        safelyRecord(record, `${baseCode}-sync`, defaultPrevented(event));
        try {
          defer(() => {
            if (active) safelyRecord(record, `${baseCode}-after-sync-dispatch`, defaultPrevented(event));
          });
        } catch {}
      };
      try { app.on(eventName, listener); listeners.push([app, eventName, listener]); }
      catch { safelyRecord(record, "lifecycle-unavailable", false); }
    }
  }

  function dispose(): void {
    if (!active) return;
    active = false;
    retireLoadHook();
    restoreGetter();
    for (const [app, event, listener] of listeners.splice(0)) {
      try { app.removeListener?.(event, listener); } catch {}
    }
  }

  return { attachLifecycle, dispose };
}
