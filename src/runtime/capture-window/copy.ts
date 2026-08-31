export type CaptureWindowCopy = {
  background: string;
  blur: string;
  changeImage: string;
  close: string;
  captureFailed: string;
  clipboardUnavailable: string;
  copied: string;
  copy: string;
  custom: string;
  maskColor: string;
  mosaic: string;
  move: string;
  padding: string;
  privacy: string;
  privacyDescription: string;
  preview: string;
  redact: string;
  regionHint: string;
  regionHintDraw: string;
  regionRemove: string;
  regionSuggestion: string;
  redo: string;
  retake: string;
  retakeFailed: string;
  save: string;
  saveFailed: string;
  saved: string;
  shadow: string;
  solid: string;
  sourceAuto: string;
  sourceAutoHint: string;
  sourceDraw: string;
  sourceDrawHint: string;
  title: string;
  tools: string;
  transparent: string;
  undo: string;
  wallpaper: string;
  wallpaperUnreadable: string;
  wallpaperTooLarge: string;
  zoomIn: string;
  zoomOut: string;
  zoomReset: string;
};

const ENGLISH: CaptureWindowCopy = {
  background: "Background",
  blur: "Blur",
  changeImage: "Change image",
  close: "Close capture window",
  captureFailed: "Unable to capture the window.",
  clipboardUnavailable: "Clipboard access is unavailable in this browser.",
  copied: "Copied to clipboard",
  copy: "Copy",
  custom: "Color",
  maskColor: "Mask color",
  mosaic: "Mosaic",
  move: "Move",
  padding: "Padding",
  privacy: "Privacy masks",
  privacyDescription: "Replace marked sensitive details before taking the screenshot.",
  preview: "Preview",
  redact: "Redact",
  regionHint: "Click detected areas to redact them",
  regionHintDraw: "Drag over any area to redact it",
  regionRemove: "Remove redaction",
  regionSuggestion: "Detected area",
  redo: "Redo",
  retake: "Retake",
  retakeFailed: "Unable to capture the window again.",
  save: "Save",
  saveFailed: "Unable to encode the PNG.",
  saved: "PNG downloaded",
  shadow: "Window shadow",
  solid: "Solid",
  sourceAuto: "Detected areas",
  sourceAutoHint: "Select detected areas",
  sourceDraw: "Draw areas",
  sourceDrawHint: "Draw custom areas",
  title: "Capture window",
  tools: "Tools",
  transparent: "Transparent",
  undo: "Undo",
  wallpaper: "Wallpaper",
  wallpaperUnreadable: "Unable to read this wallpaper.",
  wallpaperTooLarge: "Wallpaper must be PNG, JPEG, or WebP and no larger than 32 MiB.",
  zoomIn: "Zoom in",
  zoomOut: "Zoom out",
  zoomReset: "Reset zoom",
};

const CHINESE: CaptureWindowCopy = {
  background: "背景",
  blur: "模糊",
  changeImage: "更换图片",
  close: "关闭截取窗口",
  captureFailed: "无法截取窗口。",
  clipboardUnavailable: "当前浏览器无法写入剪贴板。",
  copied: "已复制到剪贴板",
  copy: "复制",
  custom: "颜色",
  maskColor: "遮罩颜色",
  mosaic: "马赛克",
  move: "移动",
  padding: "边距",
  privacy: "隐私遮罩",
  privacyDescription: "截图前将已标记的敏感信息替换为占位符。",
  preview: "预览",
  redact: "区域打码",
  regionHint: "点击检测到的区域进行打码",
  regionHintDraw: "拖动画出要打码的区域",
  regionRemove: "移除打码",
  regionSuggestion: "检测到的区域",
  redo: "重做",
  retake: "重拍",
  retakeFailed: "无法重新截取窗口。",
  save: "保存",
  saveFailed: "无法生成 PNG。",
  saved: "PNG 已下载",
  shadow: "窗口阴影",
  solid: "纯色",
  sourceAuto: "检测区域",
  sourceAutoHint: "选择检测到的区域",
  sourceDraw: "手动画框",
  sourceDrawHint: "手动画出区域",
  title: "截取窗口",
  tools: "工具",
  transparent: "透明",
  undo: "撤销",
  wallpaper: "壁纸",
  wallpaperUnreadable: "无法读取这张壁纸。",
  wallpaperTooLarge: "壁纸必须是 PNG、JPEG 或 WebP，且不超过 32 MiB。",
  zoomIn: "放大",
  zoomOut: "缩小",
  zoomReset: "重置缩放",
};

export function captureWindowCopy(locale: string): CaptureWindowCopy {
  return locale.toLowerCase().startsWith("zh") ? CHINESE : ENGLISH;
}
