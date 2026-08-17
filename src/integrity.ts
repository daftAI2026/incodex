import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { PLIST_REL } from "./paths";

const PY = `
import plistlib, sys
path, mode = sys.argv[1], sys.argv[2]
with open(path, "rb") as f:
    data = plistlib.load(f)
block = data.setdefault("ElectronAsarIntegrity", {})
entry = block.setdefault("Resources/app.asar", {"algorithm": "SHA256"})
if mode == "read":
    print(entry.get("hash") or "")
    raise SystemExit(0)
entry["algorithm"] = "SHA256"
entry["hash"] = sys.argv[3]
with open(path, "wb") as f:
    plistlib.dump(data, f, fmt=plistlib.FMT_XML)
`;

function runPython(args: string[]): string {
  const result = spawnSync("python3", ["-c", PY, ...args], { encoding: "utf8" });
  if (result.status !== 0) throw new Error(result.stderr || "plist helper failed");
  return result.stdout.trim();
}

export function readAsarIntegrity(appPath: string): string | null {
  const value = runPython([join(appPath, PLIST_REL), "read"]);
  return value || null;
}

export function writeAsarIntegrity(appPath: string, hash: string): void {
  runPython([join(appPath, PLIST_REL), "write", hash]);
}
