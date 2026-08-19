import { spawnSync, type SpawnSyncReturns } from "node:child_process";

export const LSREGISTER =
  "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

type SpawnFn = (command: string, args: string[], options: { stdio: "ignore" }) => SpawnSyncReturns<string | Buffer>;

export function notifyLaunchServices(appPath: string, spawn: SpawnFn = spawnSync): void {
  spawn(LSREGISTER, ["-f", appPath], { stdio: "ignore" });
}
