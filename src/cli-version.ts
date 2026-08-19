import pkg from "../package.json";

export function cliVersion(): string {
  return pkg.version || "0.0.0";
}
