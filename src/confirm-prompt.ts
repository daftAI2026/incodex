export const CONFIRM_PROMPT = "Press Enter to confirm, ESC to cancel: ";

export type ConfirmKey = "yes" | "no" | "interrupt";

export function interpretConfirmKey(key: string): ConfirmKey {
  if (key === "\u0003") return "interrupt";
  if (key === "\r" || key === "\n" || key === "") return "yes";
  return "no";
}

export async function askToContinue(question = CONFIRM_PROMPT): Promise<boolean> {
  const stdin = process.stdin;
  const stdout = process.stdout;
  if (!stdin.isTTY || typeof stdin.setRawMode !== "function") {
    return false;
  }

  stdout.write(question);
  stdin.setRawMode(true);
  stdin.resume();
  stdin.setEncoding("utf8");

  try {
    const key = await readOneKey(stdin);
    const result = interpretConfirmKey(key);
    stdout.write("\n");
    if (result === "interrupt") throw new Error("interrupted");
    return result === "yes";
  } finally {
    drainPendingInput(stdin);
    if (stdin.isTTY) stdin.setRawMode(false);
    stdin.pause();
  }
}

function readOneKey(stdin: NodeJS.ReadStream): Promise<string> {
  return new Promise((resolve, reject) => {
    const onData = (chunk: string | Buffer) => {
      cleanup();
      resolve(typeof chunk === "string" ? chunk : chunk.toString("utf8"));
    };
    const onError = (error: Error) => {
      cleanup();
      reject(error);
    };
    const cleanup = () => {
      stdin.off("data", onData);
      stdin.off("error", onError);
    };
    stdin.once("data", onData);
    stdin.once("error", onError);
  });
}

function drainPendingInput(stdin: NodeJS.ReadStream): void {
  stdin.resume();
  while (stdin.readableLength > 0) stdin.read();
}
