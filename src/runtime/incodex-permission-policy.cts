// @ts-nocheck
/**
 * [INPUT]: 依赖稳定 responder 传入的录屏状态枚举。
 * [OUTPUT]: 提供 CLI 请求格式校验与录屏状态解释的纯功能工厂。
 * [POS]: 主服务代际的权限能力；不持有 watcher、nonce 或进程身份。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
"use strict";

function interpretScreenPermission(observed) {
  const rawStatus = ["granted", "denied", "not-determined", "restricted"].includes(observed) ? observed : "unknown";
  return { rawStatus, status: rawStatus === "granted" ? "granted" : rawStatus === "unknown" ? "unknown" : "not-granted" };
}

function createPermissionProbePolicy() {
  return Object.freeze({ protocol: 1,
    requestIdValid: value => typeof value === "string" && /^[a-f0-9]{32}$/.test(value),
    interpretScreenPermission,
  });
}

export { createPermissionProbePolicy, interpretScreenPermission };
