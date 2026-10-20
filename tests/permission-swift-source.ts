/**
 * [INPUT]: 依赖权限 Presenter 的有序 Swift 源码。
 * [OUTPUT]: 读取同一模块并截取具名声明的花括号范围。
 * [POS]: 源码合同夹具；不依赖 private 修饰符或相邻函数位置。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";

export function readPermissionPresenterSource(root: string): string {
  return ["permission-host-presenter-geometry.swift", "permission-host-presenter-views.swift", "permission-host-presenter.swift"]
    .map(name => readFileSync(join(root, "native", "macos", name), "utf8")).join("\n");
}

// 只用于已知 Swift 夹具。不是通用解析器或产品安全校验器。
export function swiftDeclarationBody(source: string, declaration: string): string {
  const needle = declaration.replace(/^(private|fileprivate|public) /, "")
    .replace(/\([\s\S]*$/, "(").replace(/:\s*[\s\S]*$/, ":");
  const start = source.indexOf(needle);
  if (start < 0) throw new Error(`missing Swift declaration: ${declaration}`);
  const brace = source.indexOf("{", start);
  let depth = 0;
  for (let index = brace; index < source.length; index++) {
    if (source[index] === "{") depth++;
    if (source[index] === "}" && --depth === 0) return source.slice(start, index + 1);
  }
  throw new Error(`unterminated Swift declaration: ${declaration}`);
}
