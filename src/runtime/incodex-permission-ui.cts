/**
 * [INPUT]: 依赖构建器嵌入的原生呈现与动画模块。
 * [OUTPUT]: 对外提供已封装的兼容 Runtime 权限 UI。
 * [POS]: 已验证 sibling 入口；不承担权限检测或授权动作。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
// The builder embeds the native presenter and motion modules once here.
// Both the legacy Electron entry and the CLI host load this sealed sibling.
const permissionUi = "__INCODEX_ACCESSIBILITY_WINDOW__";
export = permissionUi;
