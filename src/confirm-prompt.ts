import { createInterface } from "node:readline/promises";

export async function askToContinue(question = "Continue? [y/N] "): Promise<boolean> {
  const rl = createInterface({ input: process.stdin, output: process.stdout });
  try {
    const answer = (await rl.question(question)).trim();
    return /^y(es)?$/i.test(answer);
  } finally {
    rl.close();
  }
}
