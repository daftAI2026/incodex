/**
 * [INPUT]: 依赖 Git 改动路径与 CI 环境。
 * [OUTPUT]: 输出是否需要产品检查；未知证据保留产品门。
 * [POS]: CI 选路入口；手册与明确的纯文档可跳产品检查。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { execFileSync } from "node:child_process";
import { appendFileSync } from "node:fs";

/** Unknown evidence must never turn off the product gates. */
export function requiresProductChecks(paths: string[] | null): boolean {
  return !paths?.length || paths.some(path => !path.startsWith("manual/") && !["README.md", "README_CN.md", "CONTRIBUTING.md", "SECURITY.md"].includes(path));
}

export function changedPaths(base: string, head: string, mergeBase = true): string[] | null {
  if (![base, head].every(sha => /^[a-f0-9]{40}$/.test(sha) && !/^0+$/.test(sha))) return null;
  try {
    // Disable rename folding so a move into manual/ still exposes its product source.
    const range = mergeBase ? [`${base}...${head}`] : [base, head];
    return execFileSync("git", ["diff", "--no-renames", "--name-only", "-z", ...range, "--"], {
      encoding: "utf8", stdio: ["ignore", "pipe", "pipe"],
    }).split("\0").filter(Boolean);
  } catch {
    return null;
  }
}

if (import.meta.main) {
  const paths = changedPaths(process.env.CI_DIFF_BASE ?? "", process.env.CI_DIFF_HEAD ?? "", process.env.CI_DIFF_MERGE_BASE === "true");
  const product = requiresProductChecks(paths);
  console.log(product ? "Product checks required (product paths or uncertain diff)." : "Documentation-only change; manual changes retain their dedicated validation.");
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `product=${product}\n`);
}
