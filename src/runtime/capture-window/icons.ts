/**
 * [INPUT]: 无外部依赖，使用本地 SVG 路径与调用方指定的图标语义
 * [OUTPUT]: 提供 CaptureIconName 与 captureIcon，尺寸由消费方的 Codex token 控制
 * [POS]: Shot 图标资源边界，为视图和背景动作提供统一 SVG
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
export type CaptureIconName =
  | "camera"
  | "check"
  | "chevron-down"
  | "copy"
  | "download"
  | "droplet"
  | "grid-3x3"
  | "hand"
  | "image"
  | "maximize"
  | "move"
  | "palette"
  | "pen-line"
  | "pipette"
  | "plus"
  | "redo"
  | "retake"
  | "scan"
  | "scan-search"
  | "save"
  | "shield"
  | "square"
  | "square-dashed"
  | "trash"
  | "undo"
  | "wallpaper"
  | "x"
  | "zoom-in"
  | "zoom-out";

const ICON_BODY: Record<CaptureIconName, string> = {
  camera:
    '<path d="M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3z" /><circle cx="12" cy="13" r="3" />',
  check: '<path d="M20 6 9 17l-5-5" />',
  "chevron-down": '<path d="m6 9 6 6 6-6" />',
  copy:
    '<rect width="14" height="14" x="8" y="8" rx="2" ry="2" /><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />',
  download:
    '<path d="M12 15V3" /><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="m7 10 5 5 5-5" />',
  droplet: '<path d="M12 22a7 7 0 0 0 7-7c0-2-1-3.5-3-5.5L12 3l-4 6.5C6 11.5 5 13 5 15a7 7 0 0 0 7 7z" />',
  "grid-3x3":
    '<rect width="18" height="18" x="3" y="3" rx="2" /><path d="M3 9h18" /><path d="M3 15h18" /><path d="M9 3v18" /><path d="M15 3v18" />',
  hand:
    '<path d="M18 11V6a2 2 0 0 0-4 0v5" /><path d="M14 10V4a2 2 0 0 0-4 0v7" /><path d="M10 10.5V6a2 2 0 0 0-4 0v8" /><path d="M6 14a2 2 0 1 0-4 0v2c0 4.4 3.6 8 8 8h2a8 8 0 0 0 8-8v-5a2 2 0 0 0-4 0v1" />',
  image:
    '<rect width="18" height="18" x="3" y="3" rx="2" ry="2" /><circle cx="9" cy="9" r="2" /><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21" />',
  maximize:
    '<path d="M8 3H5a2 2 0 0 0-2 2v3" /><path d="M16 3h3a2 2 0 0 1 2 2v3" /><path d="M8 21H5a2 2 0 0 1-2-2v-3" /><path d="M16 21h3a2 2 0 0 0 2-2v-3" />',
  move:
    '<path d="M12 2v20" /><path d="m15 19-3 3-3-3" /><path d="m19 9 3 3-3 3" /><path d="M2 12h20" /><path d="m5 9-3 3 3 3" /><path d="m9 5 3-3 3 3" />',
  palette:
    '<path d="M12 22a1 1 0 0 1 0-20 10 9 0 0 1 10 9 5 5 0 0 1-5 5h-2.25a1.75 1.75 0 0 0-1.4 2.8l.3.4a1.75 1.75 0 0 1-1.4 2.8z" /><circle cx="13.5" cy="6.5" r=".5" fill="currentColor" /><circle cx="17.5" cy="10.5" r=".5" fill="currentColor" /><circle cx="6.5" cy="12.5" r=".5" fill="currentColor" /><circle cx="8.5" cy="7.5" r=".5" fill="currentColor" />',
  "pen-line":
    '<path d="M13 21h8" /><path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4Z" /><path d="m15 5 3 3" />',
  pipette:
    '<path d="m2 22 1-1h3l9-9" /><path d="M3 21v-3l9-9" /><path d="m15 6 3.4-3.4a2.1 2.1 0 0 1 3 3L18 9" /><path d="m11 8 5 5" />',
  plus: '<path d="M5 12h14" /><path d="M12 5v14" />',
  redo:
    '<path d="m15 14 5-5-5-5" /><path d="M20 9H9.5A5.5 5.5 0 0 0 4 14.5 5.5 5.5 0 0 0 9.5 20H13" />',
  retake:
    '<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" /><path d="M3 3v5h5" />',
  scan:
    '<path d="M3 7V5a2 2 0 0 1 2-2h2" /><path d="M17 3h2a2 2 0 0 1 2 2v2" /><path d="M21 17v2a2 2 0 0 1-2 2h-2" /><path d="M7 21H5a2 2 0 0 1-2-2v-2" /><path d="M7 12h10" />',
  "scan-search":
    '<path d="M3 7V5a2 2 0 0 1 2-2h2" /><path d="M17 3h2a2 2 0 0 1 2 2v2" /><path d="M21 17v2a2 2 0 0 1-2 2h-2" /><path d="M7 21H5a2 2 0 0 1-2-2v-2" /><circle cx="11" cy="11" r="3" /><path d="m16 16-2.2-2.2" />',
  save:
    '<path d="M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z" /><path d="M17 21v-8H7v8" /><path d="M7 3v5h8" />',
  shield:
    '<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z" /><path d="m9 12 2 2 4-4" />',
  square: '<rect width="18" height="18" x="3" y="3" rx="2" />',
  "square-dashed":
    '<path d="M5 3a2 2 0 0 0-2 2" /><path d="M19 3a2 2 0 0 1 2 2" /><path d="M21 19a2 2 0 0 1-2 2" /><path d="M5 21a2 2 0 0 1-2-2" /><path d="M9 3h1" /><path d="M9 21h1" /><path d="M14 3h1" /><path d="M14 21h1" /><path d="M3 9v1" /><path d="M21 9v1" /><path d="M3 14v1" /><path d="M21 14v1" />',
  trash:
    '<path d="M10 11v6" /><path d="M14 11v6" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /><path d="M3 6h18" /><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />',
  undo:
    '<path d="M9 14 4 9l5-5" /><path d="M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5 5.5 5.5 0 0 1-5.5 5.5H11" />',
  wallpaper:
    '<path d="M12 17v4" /><path d="M8 21h8" /><path d="m9 17 6.1-6.1a2 2 0 0 1 2.81.01L22 15" /><circle cx="8" cy="9" r="2" /><rect x="2" y="3" width="20" height="14" rx="2" />',
  x: '<path d="M18 6 6 18" /><path d="m6 6 12 12" />',
  "zoom-in":
    '<circle cx="11" cy="11" r="8" /><line x1="21" x2="16.65" y1="21" y2="16.65" /><line x1="11" x2="11" y1="8" y2="14" /><line x1="8" x2="14" y1="11" y2="11" />',
  "zoom-out":
    '<circle cx="11" cy="11" r="8" /><line x1="21" x2="16.65" y1="21" y2="16.65" /><line x1="8" x2="14" y1="11" y2="11" />',
};

export function captureIcon(name: CaptureIconName, size = 16): string {
  return `<svg data-capture-icon="${name}" xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">${ICON_BODY[name]}</svg>`;
}
