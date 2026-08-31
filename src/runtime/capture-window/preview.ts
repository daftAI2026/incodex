import { mountCaptureWindowEditor, type CaptureWindowEditorController } from "./editor.ts";
import type { CaptureWindowState } from "./model.ts";
import { codexPreviewPrivacyRegions } from "./privacy.ts";

type PreviewTheme = "dark" | "light";
type PreviewLocale = "en" | "zh-CN";

let theme: PreviewTheme = "light";
let locale: PreviewLocale = "zh-CN";
let revision = 0;
let controller: CaptureWindowEditorController | null = null;
let preserveCloseCallback = false;

document.body.innerHTML = `
  <main class="incodex-capture-preview-shell" data-incodex-capture-preview="true">
    <div class="incodex-capture-preview-app">
      <aside class="incodex-capture-preview-sidebar">
        <strong>Codex</strong>
        <p class="incodex-capture-section-description">Preview shell</p>
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
  openEditor();
});

document.querySelector<HTMLElement>("[data-preview-theme]")?.addEventListener("click", () => {
  const state = controller?.getState();
  theme = theme === "light" ? "dark" : "light";
  applyTheme();
  remountEditor(state);
});

document.querySelector<HTMLSelectElement>("[data-preview-locale]")?.addEventListener("change", (event) => {
  const state = controller?.getState();
  locale = (event.currentTarget as HTMLSelectElement).value as PreviewLocale;
  document.documentElement.lang = locale;
  remountEditor(state);
});

applyTheme();
openEditor();

function openEditor(initialState?: CaptureWindowState): void {
  if (controller) return;
  const host = document.querySelector<HTMLElement>("[data-preview-editor]");
  if (!host) return;
  const source = createMockCodexCapture(revision, theme);
  controller = mountCaptureWindowEditor(host, {
    automaticRegions: codexPreviewPrivacyRegions(source),
    initialState,
    locale,
    onClose: () => {
      controller = null;
      if (!preserveCloseCallback) showToast("Capture editor closed");
    },
    onNotify: showToast,
    onDetectRegions: (nextSource) => codexPreviewPrivacyRegions(nextSource),
    onRetake: (nextRevision) => {
      revision = nextRevision;
      return createMockCodexCapture(revision, theme);
    },
    source,
  });
}

function remountEditor(state?: CaptureWindowState): void {
  if (!controller) return;
  preserveCloseCallback = true;
  controller.destroy();
  preserveCloseCallback = false;
  controller = null;
  openEditor(state);
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

function createMockCodexCapture(sourceRevision: number, selectedTheme: PreviewTheme): HTMLCanvasElement {
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
  context.fillText("Incodex", 28, 164);
  context.fillText("Client work", 28, 196);
  context.fillStyle = colors.muted;
  context.fillText("Recent", 28, 250);
  context.fillStyle = colors.text;
  context.fillText("Private launch workflow", 28, 282);
  context.fillText("Capture window research", 28, 314);
  context.fillStyle = "#6f8bff";
  context.beginPath();
  context.arc(34, 759, 14, 0, Math.PI * 2);
  context.fill();
  context.fillStyle = colors.text;
  context.fillText("Kid", 58, 764);

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
