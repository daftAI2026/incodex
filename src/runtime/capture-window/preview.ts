import {
  capturePreparedWindow,
  waitForCaptureFrame,
} from "./capture-lifecycle.ts";
import {
  mountCaptureWindowEditor,
  type CaptureWindowEditorController,
  type CaptureWindowRetake,
} from "./editor.ts";
import type { CaptureCandidate, CaptureWindowState } from "./model.ts";

type PreviewTheme = "dark" | "light";
type PreviewLocale = "en" | "zh-CN";

let theme: PreviewTheme = "light";
let locale: PreviewLocale = "zh-CN";
let revision = 0;
let controller: CaptureWindowEditorController | null = null;
let preserveCloseCallback = false;
let capturePending = false;

document.body.innerHTML = `
  <main class="incodex-capture-preview-shell" data-incodex-capture-preview="true">
    <div class="incodex-capture-preview-app">
      <aside class="incodex-capture-preview-sidebar">
        <strong>Codex</strong>
        <p class="incodex-capture-section-description" data-incodex-capture-redact>Preview shell</p>
      </aside>
      <section class="incodex-capture-preview-main">
        <div class="incodex-capture-preview-card">
          <h1 class="incodex-capture-title">Capture Window</h1>
          <p class="incodex-capture-section-description">The preview uses the same semantic token bridge and editor components intended for the injected Runtime.</p>
        </div>
        <button class="incodex-capture-button incodex-capture-button-primary" data-preview-open type="button">Open capture window</button>
      </section>
    </div>
    <div class="incodex-capture-preview-controls">
      <select class="incodex-capture-button incodex-capture-button-secondary" data-preview-locale aria-label="Preview locale">
        <option value="zh-CN">中文</option>
        <option value="en">English</option>
      </select>
      <button class="incodex-capture-button incodex-capture-button-secondary" data-preview-theme type="button">Dark</button>
    </div>
    <div data-preview-editor></div>
    <div data-preview-toast aria-live="polite"></div>
  </main>
`;

document.querySelector<HTMLElement>("[data-preview-open]")?.addEventListener("click", () => {
  void openEditor();
});

document.querySelector<HTMLElement>("[data-preview-theme]")?.addEventListener("click", () => {
  const state = controller?.getState();
  theme = theme === "light" ? "dark" : "light";
  applyTheme();
  void remountEditor(state);
});

document.querySelector<HTMLSelectElement>("[data-preview-locale]")?.addEventListener("change", (event) => {
  const state = controller?.getState();
  locale = (event.currentTarget as HTMLSelectElement).value as PreviewLocale;
  document.documentElement.lang = locale;
  void remountEditor(state);
});

applyTheme();

async function openEditor(initialState?: CaptureWindowState): Promise<void> {
  if (controller || capturePending) return;
  const host = document.querySelector<HTMLElement>("[data-preview-editor]");
  if (!host) return;
  capturePending = true;
  try {
    const privacyEnabled = initialState?.privacyEnabled ?? true;
    const snapshot = await capturePreviewSnapshot(revision, privacyEnabled);
    controller = mountCaptureWindowEditor(host, {
      automaticRegions: snapshot.automaticRegions,
      initialState,
      locale,
      onClose: () => {
        controller = null;
        if (!preserveCloseCallback) showToast("Capture editor closed");
      },
      onNotify: showToast,
      onRetake: (nextRevision, nextPrivacyEnabled) => {
        revision = nextRevision;
        return capturePreviewSnapshot(revision, nextPrivacyEnabled);
      },
      source: snapshot.source,
    });
  } finally {
    capturePending = false;
  }
}

async function remountEditor(state?: CaptureWindowState): Promise<void> {
  if (!controller) return;
  preserveCloseCallback = true;
  controller.destroy();
  preserveCloseCallback = false;
  controller = null;
  await openEditor(state);
}

function applyTheme(): void {
  document.documentElement.classList.toggle("electron-dark", theme === "dark");
  document.documentElement.classList.toggle("electron-light", theme === "light");
  const button = document.querySelector<HTMLElement>("[data-preview-theme]");
  if (button) button.textContent = theme === "light" ? "Dark" : "Light";
}

function showToast(message: string): void {
  const region = document.querySelector<HTMLElement>("[data-preview-toast]");
  if (!region) return;
  region.innerHTML = `<div class="incodex-capture-toast">${message}</div>`;
  window.setTimeout(() => {
    region.replaceChildren();
  }, 2400);
}

async function capturePreviewSnapshot(
  sourceRevision: number,
  privacyEnabled: boolean,
): Promise<CaptureWindowRetake> {
  const snapshot = await capturePreparedWindow({
    capture: () => createMockCodexCapture(sourceRevision, theme, privacyEnabled),
    collectCandidates: () => createMockCaptureCandidates(privacyEnabled),
    privacyEnabled,
    root: document.documentElement,
    waitForFrame: waitForCaptureFrame,
  });
  return {
    automaticRegions: snapshot.candidates,
    source: snapshot.source,
  };
}

function createMockCodexCapture(
  sourceRevision: number,
  selectedTheme: PreviewTheme,
  privacyEnabled: boolean,
): HTMLCanvasElement {
  const canvas = document.createElement("canvas");
  canvas.width = 1200;
  canvas.height = 801;
  const context = canvas.getContext("2d");
  if (!context) return canvas;

  const dark = selectedTheme === "dark";
  const colors = {
    border: dark ? "#36383c" : "#dddddd",
    bubble: dark ? "#2c2e32" : "#f3f3f3",
    muted: dark ? "#969ba2" : "#7c8188",
    sidebar: dark ? "#1b1c1e" : "#eeeeee",
    surface: dark ? "#202123" : "#ffffff",
    text: dark ? "#f2f2f2" : "#202123",
  };
  context.fillStyle = colors.surface;
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.fillStyle = colors.sidebar;
  context.fillRect(0, 0, 248, canvas.height);
  context.strokeStyle = colors.border;
  context.beginPath();
  context.moveTo(248.5, 0);
  context.lineTo(248.5, canvas.height);
  context.stroke();

  context.fillStyle = colors.text;
  context.font = "600 22px -apple-system, BlinkMacSystemFont, sans-serif";
  context.fillText("Codex", 34, 50);
  context.font = "14px -apple-system, BlinkMacSystemFont, sans-serif";
  context.fillText("＋  New thread", 28, 98);
  context.fillStyle = colors.muted;
  context.fillText("Projects", 28, 132);
  context.fillStyle = colors.text;
  drawPrivateText(context, "Incodex", 28, 164, privacyEnabled, colors.text);
  drawPrivateText(context, "Client work", 28, 196, privacyEnabled, colors.text);
  context.fillStyle = colors.muted;
  context.fillText("Recent", 28, 250);
  context.fillStyle = colors.text;
  drawPrivateText(context, "Private launch workflow", 28, 282, privacyEnabled, colors.text);
  drawPrivateText(context, "Capture window research", 28, 314, privacyEnabled, colors.text);
  context.fillStyle = "#6f8bff";
  context.beginPath();
  context.arc(34, 759, 14, 0, Math.PI * 2);
  context.fill();
  context.fillStyle = colors.text;
  drawPrivateText(context, "Kid", 58, 764, privacyEnabled, colors.text);

  context.fillStyle = colors.text;
  context.font = "600 20px -apple-system, BlinkMacSystemFont, sans-serif";
  context.fillText("Capture window implementation", 438, 112);
  context.fillStyle = colors.muted;
  context.font = "13px -apple-system, BlinkMacSystemFont, sans-serif";
  context.fillText(`Mock Codex window · retake ${sourceRevision}`, 438, 140);

  roundRect(context, 402, 252, 548, 102, 18);
  context.fillStyle = colors.bubble;
  context.fill();
  context.fillStyle = colors.text;
  context.font = "15px -apple-system, BlinkMacSystemFont, sans-serif";
  context.fillText("Please mask my account details and this project path", 430, 294);
  context.fillText("/Users/luo/Desktop/ClaudeCode/web/incodex", 430, 326);

  context.fillStyle = colors.text;
  context.font = "15px -apple-system, BlinkMacSystemFont, sans-serif";
  context.fillText("I’ll prepare a share-ready image and preserve your privacy choices.", 350, 440);
  context.fillStyle = colors.muted;
  context.fillText("Automatic masks are candidates — review them before sharing.", 350, 472);

  roundRect(context, 350, 650, 602, 92, 24);
  context.fillStyle = colors.bubble;
  context.fill();
  context.strokeStyle = colors.border;
  context.stroke();
  context.fillStyle = colors.muted;
  context.fillText("Ask Codex anything", 382, 704);
  return canvas;
}

function createMockCaptureCandidates(privacyEnabled: boolean): CaptureCandidate[] {
  const publicCandidates: CaptureCandidate[] = [
    { height: 28, id: "r:438:92:330:28", width: 330, x: 438, y: 92 },
    { height: 54, id: "r:430:277:474:54", width: 474, x: 430, y: 277 },
    { height: 24, id: "r:350:425:555:24", width: 555, x: 350, y: 425 },
    { height: 40, id: "r:382:680:320:40", width: 320, x: 382, y: 680 },
  ];
  if (privacyEnabled) return publicCandidates;
  return [
    { height: 24, id: "r:28:145:190:24", width: 190, x: 28, y: 145 },
    { height: 24, id: "r:28:177:190:24", width: 190, x: 28, y: 177 },
    { height: 24, id: "r:28:263:205:24", width: 205, x: 28, y: 263 },
    { height: 24, id: "r:28:295:205:24", width: 205, x: 28, y: 295 },
    { height: 28, id: "r:58:744:120:28", width: 120, x: 58, y: 744 },
    ...publicCandidates,
  ];
}

function drawPrivateText(
  context: CanvasRenderingContext2D,
  text: string,
  x: number,
  y: number,
  privacyEnabled: boolean,
  color: string,
): void {
  if (!privacyEnabled) {
    context.fillText(text, x, y);
    return;
  }
  context.save();
  context.fillStyle = color;
  context.globalAlpha = 0.18;
  context.beginPath();
  context.roundRect(x, y - 9, Math.max(54, context.measureText(text).width * 0.62), 8, 999);
  context.fill();
  context.restore();
}

function roundRect(
  context: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number,
): void {
  context.beginPath();
  context.roundRect(x, y, width, height, radius);
}
