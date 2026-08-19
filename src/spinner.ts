const FRAMES = ["|", "/", "-", "\\"];
const PREFIX = "  ";

export type Spinner = { stop: () => void };

export type SpinnerTimer = ReturnType<typeof setInterval>;

export type SpinnerOptions = {
  tty?: boolean;
  write?: (text: string) => void;
  setInterval?: (tick: () => void, ms: number) => SpinnerTimer;
  clearInterval?: (timer: SpinnerTimer) => void;
  intervalMs?: number;
};

export function startSpinner(message: string, options: SpinnerOptions = {}): Spinner {
  const tty = options.tty ?? Boolean(process.stderr.isTTY);
  const write = options.write ?? ((text: string) => process.stderr.write(text));
  if (!tty) return { stop() {} };

  let frame = 0;
  const draw = () => {
    write(`\r\u001b[2K${PREFIX}${FRAMES[frame % FRAMES.length]} ${message}`);
    frame += 1;
  };
  draw();
  const timer = (options.setInterval ?? setInterval)(draw, options.intervalMs ?? 50);
  return {
    stop() {
      (options.clearInterval ?? clearInterval)(timer);
      write("\r\u001b[2K");
    },
  };
}

export async function withSpinner<T>(message: string, fn: () => Promise<T>, options?: SpinnerOptions): Promise<T> {
  const spinner = startSpinner(message, options);
  try {
    return await fn();
  } finally {
    spinner.stop();
  }
}
