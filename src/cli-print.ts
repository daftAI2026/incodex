const LABEL_WIDTH = 12;

const ANSI = {
  green: "0;32",
  yellow: "0;33",
  purple: "1;35",
  gray: "0;38;5;244",
} as const;

export type PrintOptions = { color?: boolean };

function useColor(options?: PrintOptions): boolean {
  return options?.color ?? Boolean(process.stdout.isTTY);
}

function paint(enabled: boolean, code: string, text: string): string {
  if (!enabled) return text;
  return `\u001b[${code}m${text}\u001b[0m`;
}

export function formatKv(label: string, value: string, options?: PrintOptions): string {
  const color = useColor(options);
  return `  ${paint(color, ANSI.gray, label.padEnd(LABEL_WIDTH))} ${value}`;
}

export function formatStep(message: string, options?: PrintOptions): string {
  return `${paint(useColor(options), ANSI.purple, "➤")} ${message}`;
}

export function formatSection(title: string, options?: PrintOptions): string {
  return formatStep(title, options);
}

export function formatOk(message: string, options?: PrintOptions): string {
  return `  ${paint(useColor(options), ANSI.green, "✓")} ${message}`;
}

export function formatWarn(message: string, options?: PrintOptions): string {
  return `  ${paint(useColor(options), ANSI.yellow, "!")} ${message}`;
}

export function formatHint(message: string, options?: PrintOptions): string {
  return paint(useColor(options), ANSI.gray, message);
}
