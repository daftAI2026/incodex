import { createInterface } from "node:readline/promises";
import type { CliCommand } from "./parse-cli";

export type MenuChoice = Exclude<CliCommand, "menu" | "help" | "version" | "runtime" | "recover"> | "quit";

export type MenuItem = {
  id: MenuChoice;
  label: string;
};

export const MENU_ITEMS: MenuItem[] = [
  { id: "install", label: "Install into the Codex app you are using" },
  { id: "uninstall", label: "Remove Incodex from Codex" },
  { id: "open", label: "Open an incognito window (does not patch Codex)" },
  { id: "status", label: "Show status" },
  { id: "doctor", label: "Diagnose" },
  { id: "quit", label: "Quit" },
];

export async function runMenu(): Promise<MenuChoice> {
  if (canUseRawMenu()) {
    try {
      return await rawMenu();
    } catch {
      /* fall through to numbered prompt */
    }
  }
  return numberedMenu();
}

function canUseRawMenu(): boolean {
  return Boolean(process.stdin.isTTY && process.stdout.isTTY && typeof process.stdin.setRawMode === "function");
}

function render(selected: number): string {
  const lines = ["incodex", "", ...MENU_ITEMS.map((item, index) => {
    const mark = index === selected ? ">" : " ";
    return `${mark} ${index + 1}. ${item.label}`;
  }), "", "↑↓ or 1-6, Enter to choose, q to quit"];
  return lines.join("\n");
}

async function rawMenu(): Promise<MenuChoice> {
  let selected = 0;
  let painted = 0;
  const stdin = process.stdin;
  stdin.setRawMode(true);
  stdin.resume();
  stdin.setEncoding("utf8");

  const paint = () => {
    if (painted > 0) process.stdout.write(`\u001b[${painted}A\u001b[J`);
    const text = render(selected);
    process.stdout.write(`${text}\n`);
    painted = text.split("\n").length;
  };

  return new Promise((resolve, reject) => {
    const finish = (choice: MenuChoice) => {
      cleanup();
      resolve(choice);
    };
    const onData = (chunk: string | Buffer) => {
      const key = typeof chunk === "string" ? chunk : chunk.toString("utf8");
      if (key === "\u0003") {
        cleanup();
        reject(new Error("interrupted"));
        return;
      }
      if (key === "q" || key === "Q" || key === "\u001b") {
        finish("quit");
        return;
      }
      if (key === "\r" || key === "\n") {
        finish(MENU_ITEMS[selected]?.id ?? "quit");
        return;
      }
      if (key === "\u001b[A" || key === "k") {
        selected = (selected + MENU_ITEMS.length - 1) % MENU_ITEMS.length;
        paint();
        return;
      }
      if (key === "\u001b[B" || key === "j") {
        selected = (selected + 1) % MENU_ITEMS.length;
        paint();
        return;
      }
      if (key.length === 1 && key >= "1" && key <= "9") {
        const index = Number(key) - 1;
        if (MENU_ITEMS[index]) finish(MENU_ITEMS[index].id);
      }
    };
    const cleanup = () => {
      stdin.off("data", onData);
      if (stdin.isTTY) stdin.setRawMode(false);
      stdin.pause();
    };
    try {
      paint();
      stdin.on("data", onData);
    } catch (error) {
      cleanup();
      reject(error);
    }
  });
}

async function numberedMenu(): Promise<MenuChoice> {
  process.stdout.write(`${render(0)}\n`);
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  try {
    const answer = (await rl.question("Choose [1-6]: ")).trim().toLowerCase();
    if (answer === "" || answer === "q") return "quit";
    const index = Number(answer) - 1;
    return MENU_ITEMS[index]?.id ?? "quit";
  } finally {
    rl.close();
  }
}
