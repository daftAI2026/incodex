import { createInterface } from "node:readline/promises";
import type { CliCommand } from "./parse-cli";

export type MenuChoice = Exclude<CliCommand, "menu" | "help" | "runtime" | "recover"> | "quit";

export type MenuItem = {
  id: MenuChoice;
  title: string;
  description: string;
};

export type RenderMenuOptions = {
  color?: boolean;
  updateMessage?: string;
};

export type MenuKeyResult =
  | { action: "select"; id: MenuChoice }
  | { action: "move"; selected: number }
  | { action: "interrupt" }
  | { action: "ignore" };

export const MENU_REPO_URL = "https://github.com/daftAI2026/incodex";
export const MENU_TAGLINE = "Incognito toggle for Codex desktop.";
export const MENU_ARROW = "➤";

export const MENU_BANNER = [
  "  _____   _   _    _____    ____    _____    ______  __   __",
  " |_   _| | \\ | |  / ____|  / __ \\  |  __ \\  |  ____| \\ \\ / /",
  "   | |   |  \\| | | |      | |  | | | |  | | | |__     \\ V /",
  "   | |   | . ` | | |      | |  | | | |  | | |  __|     > <",
  "  _| |_  | |\\  | | |____  | |__| | | |__| | | |____   / . \\",
  " |_____| |_| \\_|  \\_____|  \\____/  |_____/  |______| /_/ \\_\\",
].join("\n");

export const MENU_ITEMS: MenuItem[] = [
  { id: "install", title: "Install", description: "Patch the Codex app you are using" },
  { id: "uninstall", title: "Uninstall", description: "Restore the official Codex app" },
  { id: "open", title: "Open", description: "Open an incognito window without patching" },
  { id: "status", title: "Status", description: "Show whether Incodex is installed" },
  { id: "doctor", title: "Doctor", description: "Diagnose the install and leftover sessions" },
  { id: "quit", title: "Quit", description: "Exit this menu" },
];

export async function runMenu(options: RenderMenuOptions = {}): Promise<MenuChoice> {
  if (canUseRawMenu()) {
    try {
      return await rawMenu(options);
    } catch {
      /* fall through to numbered prompt */
    }
  }
  return numberedMenu(options);
}

function canUseRawMenu(): boolean {
  return Boolean(process.stdin.isTTY && process.stdout.isTTY && typeof process.stdin.setRawMode === "function");
}

const ANSI = {
  green: "0;32",
  blue: "1;34",
  cyan: "0;36",
  gray: "0;38;5;244",
} as const;

function ansi(enabled: boolean, code: string, text: string): string {
  if (!enabled) return text;
  return `\u001b[${code}m${text}\u001b[0m`;
}

function leadingSpaces(text: string): number {
  const index = text.search(/\S/);
  return index < 0 ? 0 : index;
}

function alignWithWordmark(text: string, indent: number): string {
  return `${" ".repeat(indent)}${text}`;
}

export function menuControlsLine(updateAvailable: boolean): string {
  const parts = ["↑↓", "Enter"];
  if (updateAvailable) parts.push("U Update");
  parts.push("V Version", "Q Quit", `1-${MENU_ITEMS.length} Jump`);
  return parts.join(" | ");
}

export function handleMenuKey(
  key: string,
  selected: number,
  options: { updateAvailable?: boolean } = {},
): MenuKeyResult {
  if (key === "\u0003") return { action: "interrupt" };
  if (key === "\u001b[A" || key === "k" || key === "K") {
    return { action: "move", selected: (selected + MENU_ITEMS.length - 1) % MENU_ITEMS.length };
  }
  if (key === "\u001b[B" || key === "j" || key === "J") {
    return { action: "move", selected: (selected + 1) % MENU_ITEMS.length };
  }
  if (key === "q" || key === "Q" || key === "\u001b") return { action: "select", id: "quit" };
  if (key === "v" || key === "V") return { action: "select", id: "version" };
  if (key === "\r" || key === "\n") return { action: "select", id: MENU_ITEMS[selected]?.id ?? "quit" };
  if (key.length === 1 && key >= "1" && key <= "9") {
    const item = MENU_ITEMS[Number(key) - 1];
    if (item) return { action: "select", id: item.id };
    return { action: "ignore" };
  }
  if ((key === "u" || key === "U") && options.updateAvailable) return { action: "select", id: "update" };
  return { action: "ignore" };
}

export function renderMenu(selected: number, options: RenderMenuOptions = {}): string {
  const color = options.color ?? Boolean(process.stdout.isTTY);
  const bannerLines = MENU_BANNER.split("\n");
  const indent = leadingSpaces(bannerLines[0] ?? "");
  const titleWidth = Math.max(...MENU_ITEMS.map((item) => item.title.length));
  const updateAvailable = Boolean(options.updateMessage);
  const lines = [
    ...bannerLines.map((row) => ansi(color, ANSI.green, row)),
    "",
    ansi(color, ANSI.blue, alignWithWordmark(MENU_REPO_URL, indent)),
    ansi(color, ANSI.green, alignWithWordmark(MENU_TAGLINE, indent)),
  ];
  if (options.updateMessage) {
    lines.push("", ansi(color, ANSI.green, alignWithWordmark(options.updateMessage, indent)));
  }
  lines.push("");
  for (const [index, item] of MENU_ITEMS.entries()) {
    const body = `${index + 1}. ${item.title.padEnd(titleWidth)}  ${item.description}`;
    if (index === selected) {
      lines.push(ansi(color, ANSI.cyan, `${MENU_ARROW} ${body}`));
    } else {
      lines.push(`  ${body}`);
    }
  }
  lines.push("");
  lines.push(ansi(color, ANSI.gray, menuControlsLine(updateAvailable)));
  return lines.join("\n");
}

export const HIDE_CURSOR = "\u001b[?25l";
export const SHOW_CURSOR = "\u001b[?25h";
export const MENU_HOME = "\u001b[H";
export const ERASE_DOWN = "\u001b[J";
export const CLEAR_LINE = "\r\u001b[2K";

export function erasePaintedLines(count: number): string {
  if (count <= 0) return "";
  return `\u001b[${count}A\u001b[J`;
}

export function drainPendingInput(stdin: NodeJS.ReadStream = process.stdin): void {
  stdin.resume();
  while (stdin.readableLength > 0) stdin.read();
}

async function rawMenu(options: RenderMenuOptions): Promise<MenuChoice> {
  let selected = 0;
  const stdin = process.stdin;
  stdin.setRawMode(true);
  stdin.resume();
  stdin.setEncoding("utf8");
  process.stdout.write(HIDE_CURSOR);

  const draw = () => {
    const text = renderMenu(selected, options);
    const framed = text
      .split("\n")
      .map((line) => `${CLEAR_LINE}${line}`)
      .join("\n");
    process.stdout.write(`${MENU_HOME}${framed}\n${ERASE_DOWN}`);
  };

  return new Promise((resolve, reject) => {
    const finish = (choice: MenuChoice) => {
      cleanup();
      resolve(choice);
    };
    const onData = (chunk: string | Buffer) => {
      const key = typeof chunk === "string" ? chunk : chunk.toString("utf8");
      const result = handleMenuKey(key, selected, { updateAvailable: Boolean(options.updateMessage) });
      if (result.action === "interrupt") {
        cleanup();
        reject(new Error("interrupted"));
        return;
      }
      if (result.action === "select") {
        finish(result.id);
        return;
      }
      if (result.action === "move") {
        selected = result.selected;
        draw();
      }
    };
    const cleanup = () => {
      stdin.off("data", onData);
      drainPendingInput(stdin);
      if (stdin.isTTY) stdin.setRawMode(false);
      process.stdout.write(`${MENU_HOME}${ERASE_DOWN}${SHOW_CURSOR}`);
      stdin.pause();
    };
    try {
      draw();
      stdin.on("data", onData);
    } catch (error) {
      cleanup();
      reject(error);
    }
  });
}

async function numberedMenu(options: RenderMenuOptions): Promise<MenuChoice> {
  process.stdout.write(`${renderMenu(0, options)}\n`);
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  try {
    const answer = (await rl.question(`Choose [1-${MENU_ITEMS.length}]: `)).trim().toLowerCase();
    if (answer === "" || answer === "q") return "quit";
    if (answer === "u" && options.updateMessage) return "update";
    if (answer === "v") return "version";
    const index = Number(answer) - 1;
    return MENU_ITEMS[index]?.id ?? "quit";
  } finally {
    rl.close();
  }
}
