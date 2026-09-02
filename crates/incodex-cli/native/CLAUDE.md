# native/
> L2 | 父级: ../CLAUDE.md

## 成员清单

`macos_sparkle_interpose.m`: Sparkle 初始化窄缝 interposer，仅把更新生命周期的 application bundle 交给外置 Coordinator。
`macos_update_coordinator.m`: 更新状态机宿主，以 handoff 所有权约束 pending 清理，并在执行前校验当前注册的内容寻址 Helper；后台换包后静默恢复且不擅自重开应用。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
