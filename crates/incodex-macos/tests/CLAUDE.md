# tests/
> L2 | 父级: ../CLAUDE.md

## 成员清单

`add_load_dylib.rs`: Mach-O 注入回归，验证新增加载命令不破坏段布局与已有 load command。
`quiescence.rs`: 应用静止态回归，约束 mutation 前的进程发现与拒绝策略。
`session_processes.rs`: 隔离会话进程回归，验证 session owner 的平台级识别边界。
`signing_components.rs`: 组件签名拓扑回归，验证 CUA/vendor sidecar 保留，同时约束 Sparkle、Provider Framework 与 Electron helpers 加入宿主 ad-hoc 身份。
`signing_policy.rs`: entitlement 与签名身份策略回归，约束 ad-hoc 权限裁剪和官方身份判定。
`signing_regressions.rs`: 历史签名事故回归，覆盖损坏签名、未知组件与 deep/strict 验收失败。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
