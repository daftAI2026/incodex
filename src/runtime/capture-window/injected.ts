import {
  capturePreparedWindow,
  prepareCaptureWindow,
  waitForCaptureFrame,
} from "./capture-lifecycle.ts";
import { createCaptureCdpBridge, type CaptureDebugResponse } from "./cdp-bridge.ts";
import {
  mountCaptureWindowEditor,
  type CaptureWindowEditorController,
  type CaptureWindowRetake,
} from "./editor.ts";
import { createCaptureWindowState } from "./model.ts";
import { applyCapturePreferences, loadCapturePreferences } from "./preferences.ts";
import {
  collectCaptureCandidates,
  markCodexPrivacyPlaceholders,
} from "./privacy.ts";
import { configureCapturePresetAssets } from "./presets.ts";

export type InjectedCaptureWindowOptions = {
  locale?: string;
  presetAssets: Record<string, string>;
  styleText: string;
};

const STYLE_ID = "incodex-capture-window-style";
const HOST_ATTRIBUTE = "data-incodex-capture-host";
const bridge = createCaptureCdpBridge();
let controller: CaptureWindowEditorController | null = null;
let opening = false;

export function openInjectedCaptureWindow(options: InjectedCaptureWindowOptions): void {
  if (controller || opening) return;
  installCaptureAssets(options);
  exposeCaptureBridge();
  opening = true;
  void prepareInitialCapture(options).finally(() => {
    opening = false;
  });
}

async function prepareInitialCapture(options: InjectedCaptureWindowOptions): Promise<void> {
  const prepared = await prepareCaptureWindow({
    capture: captureSnapshot,
    loadPreferences: () => loadCapturePreferences(window.localStorage),
  });
  if (!prepared) return;

  const host = captureHost();
  const snapshot = prepared.snapshot;
  const state = applyCapturePreferences(
    createCaptureWindowState({
      height: snapshot.source.height,
      scaleFactor: window.devicePixelRatio || 1,
      width: snapshot.source.width,
    }),
    prepared.preferences,
  );
  controller = mountCaptureWindowEditor(host, {
    automaticRegions: snapshot.automaticRegions,
    initialState: state,
    locale: options.locale,
    onClose: () => {
      controller = null;
      host.remove();
    },
    onRetake: (_revision, privacyEnabled) => captureSnapshot(privacyEnabled),
    source: snapshot.source,
  });
}

async function captureSnapshot(privacyEnabled: boolean): Promise<CaptureWindowRetake> {
  const restorePrivacyPlaceholders = privacyEnabled
    ? markCodexPrivacyPlaceholders(document)
    : () => {};
  try {
    const snapshot = await capturePreparedWindow({
      capture: async () => imageDataUrlToCanvas(await bridge.capture()),
      collectCandidates: () => collectCaptureCandidates(document, viewportSize()),
      privacyEnabled,
      root: document.documentElement,
      waitForFrame: waitForCaptureFrame,
    });
    return {
      automaticRegions: snapshot.candidates,
      source: snapshot.source,
    };
  } finally {
    restorePrivacyPlaceholders();
  }
}

function exposeCaptureBridge(): void {
  window.__incodexTakeCaptureDebugRequest = bridge.takeRequest;
  window.__incodexResolveCaptureDebug = bridge.resolve;
}

function installCaptureAssets(options: InjectedCaptureWindowOptions): void {
  configureCapturePresetAssets(options.presetAssets);
  if (document.getElementById(STYLE_ID)) return;
  const style = document.createElement("style");
  style.id = STYLE_ID;
  style.textContent = options.styleText;
  document.head.append(style);
}

function captureHost(): HTMLElement {
  const existing = document.querySelector<HTMLElement>(`[${HOST_ATTRIBUTE}]`);
  if (existing) return existing;
  const host = document.createElement("div");
  host.setAttribute(HOST_ATTRIBUTE, "");
  host.setAttribute("data-incodex-capture-hide", "");
  document.body.append(host);
  return host;
}

function viewportSize(): { height: number; width: number } {
  return {
    height: document.documentElement.clientHeight,
    width: document.documentElement.clientWidth,
  };
}

function imageDataUrlToCanvas(dataUrl: string): Promise<HTMLCanvasElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => {
      const canvas = document.createElement("canvas");
      canvas.width = image.naturalWidth;
      canvas.height = image.naturalHeight;
      const context = canvas.getContext("2d");
      if (!context) {
        reject(new Error("Unable to create capture canvas"));
        return;
      }
      context.drawImage(image, 0, 0);
      resolve(canvas);
    };
    image.onerror = () => reject(new Error("Unable to decode captured PNG"));
    image.src = dataUrl;
  });
}

declare global {
  interface Window {
    __incodexResolveCaptureDebug?: (response: CaptureDebugResponse) => boolean;
    __incodexTakeCaptureDebugRequest?: () => { id: string; kind: "capture" } | null;
  }
}
