export type CaptureWindowCopy = {
  automatic: string;
  automaticBadge: string;
  background: string;
  blur: string;
  cancel: string;
  clear: string;
  close: string;
  clipboardUnavailable: string;
  copied: string;
  copy: string;
  custom: string;
  layout: string;
  manualReview: string;
  maskColor: string;
  mosaic: string;
  move: string;
  padding: string;
  privacy: string;
  privacyDescription: string;
  preview: string;
  redact: string;
  redactionStyle: string;
  redo: string;
  retake: string;
  retakeFailed: string;
  save: string;
  saveFailed: string;
  saved: string;
  shadow: string;
  solid: string;
  subtitle: string;
  title: string;
  tools: string;
  transparent: string;
  undo: string;
  wallpaper: string;
  wallpaperUnreadable: string;
  wallpaperTooLarge: string;
  zoomIn: string;
  zoomOut: string;
};

const ENGLISH: CaptureWindowCopy = {
  automatic: "automatic areas",
  automaticBadge: "Auto",
  background: "Background",
  blur: "Blur",
  cancel: "Cancel",
  clear: "Clear",
  close: "Close capture window",
  clipboardUnavailable: "Clipboard access is unavailable in this browser.",
  copied: "Copied to clipboard",
  copy: "Copy",
  custom: "Color",
  layout: "Layout",
  manualReview: "Review every mask before sharing.",
  maskColor: "Mask color",
  mosaic: "Mosaic",
  move: "Move",
  padding: "Padding",
  privacy: "Privacy masks",
  privacyDescription: "Find common sensitive areas. Add or remove masks before export.",
  preview: "Preview",
  redact: "Redact",
  redactionStyle: "Redaction style",
  redo: "Redo",
  retake: "Retake",
  retakeFailed: "Unable to capture the window again.",
  save: "Save",
  saveFailed: "Unable to encode the PNG.",
  saved: "PNG downloaded",
  shadow: "Window shadow",
  solid: "Solid",
  subtitle: "Mask sensitive details, then copy or save a share-ready PNG.",
  title: "Capture window",
  tools: "Tools",
  transparent: "Transparent",
  undo: "Undo",
  wallpaper: "Wallpaper",
  wallpaperUnreadable: "Unable to read this wallpaper.",
  wallpaperTooLarge: "Wallpaper must be PNG, JPEG, or WebP and no larger than 32 MiB.",
  zoomIn: "Zoom in",
  zoomOut: "Zoom out",
};

const CHINESE: CaptureWindowCopy = {
  automatic: "个自动区域",
  automaticBadge: "自动",
  background: "背景",
  blur: "模糊",
  cancel: "取消",
  clear: "清空",
  close: "关闭截取窗口",
  clipboardUnavailable: "当前浏览器无法写入剪贴板。",
  copied: "已复制到剪贴板",
  copy: "复制",
  custom: "颜色",
  layout: "布局",
  manualReview: "分享前请检查每一个遮罩区域。",
  maskColor: "遮罩颜色",
  mosaic: "马赛克",
  move: "移动",
  padding: "边距",
  privacy: "隐私遮罩",
  privacyDescription: "发现常见敏感区域；导出前仍可手工补充或清理遮罩。",
  preview: "预览",
  redact: "区域打码",
  redactionStyle: "遮罩样式",
  redo: "重做",
  retake: "重拍",
  retakeFailed: "无法重新截取窗口。",
  save: "保存",
  saveFailed: "无法生成 PNG。",
  saved: "PNG 已下载",
  shadow: "窗口阴影",
  solid: "纯色",
  subtitle: "遮住敏感信息，再复制或保存为适合分享的 PNG。",
  title: "截取窗口",
  tools: "工具",
  transparent: "透明",
  undo: "撤销",
  wallpaper: "壁纸",
  wallpaperUnreadable: "无法读取这张壁纸。",
  wallpaperTooLarge: "壁纸必须是 PNG、JPEG 或 WebP，且不超过 32 MiB。",
  zoomIn: "放大",
  zoomOut: "缩小",
};

export function captureWindowCopy(locale: string): CaptureWindowCopy {
  return locale.toLowerCase().startsWith("zh") ? CHINESE : ENGLISH;
}
