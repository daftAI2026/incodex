import { dirname, join } from "node:path";

export type InstallChannel = "source" | "script" | "homebrew";

export type UpdateAction = { kind: "refuse"; message: string };

export function detectInstallChannel(input: { execPath: string; argv1: string }): InstallChannel {
  if (input.argv1.endsWith(".ts") || input.argv1.endsWith(".cts")) return "source";
  const hay = `${input.execPath}\n${input.argv1}`;
  if (/\/Cellar\/incodex\/|\/opt\/homebrew\/(?:opt\/)?incodex\/|\/opt\/homebrew\/bin\/inc(?:odex)?$/.test(hay)) {
    return "homebrew";
  }
  return "script";
}

export function updateAction(channel: InstallChannel): UpdateAction {
  if (channel === "homebrew") {
    return { kind: "refuse", message: "this copy was installed with Homebrew\n  brew upgrade incodex" };
  }
  if (channel === "source") {
    return {
      kind: "refuse",
      message: "this copy is running from source\n  git pull && bun install --frozen-lockfile && bun link",
    };
  }
  return {
    kind: "refuse",
    message: "the legacy TypeScript CLI updater has been retired\n  install the current Rust CLI from the latest release",
  };
}

export function selfUninstallPaths(execPath: string): string[] {
  const dir = dirname(execPath);
  return [join(dir, "incodex"), join(dir, "inc")];
}
