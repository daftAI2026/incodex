const LABEL_WIDTH = 12;

export function formatKv(label: string, value: string): string {
  return `  ${label.padEnd(LABEL_WIDTH)} ${value}`;
}

export function formatSection(title: string): string {
  return title;
}

export function formatOk(message: string): string {
  return `✓ ${message}`;
}

export function formatWarn(message: string): string {
  return `! ${message}`;
}

export function formatStep(message: string): string {
  return `➤ ${message}`;
}
