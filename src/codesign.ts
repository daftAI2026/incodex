import { spawnSync } from "node:child_process";

export function signApp(appPath: string): void {
  const result = spawnSync(
    "codesign",
    ["--force", "--deep", "--sign", "-", appPath],
    { encoding: "utf8" },
  );
  if (result.status !== 0) {
    throw new Error(result.stderr || "codesign failed");
  }
}

export function verifyApp(appPath: string): boolean {
  const result = spawnSync("codesign", ["--verify", "--deep", "--strict", appPath], {
    encoding: "utf8",
  });
  return result.status === 0;
}
