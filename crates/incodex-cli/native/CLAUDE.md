# native/
> L2 | 父级: ../CLAUDE.md

## 成员清单

`macos_sparkle_interpose.m`: Sparkle 初始化窄缝 interposer，仅把更新生命周期的 application bundle 交给外置 Coordinator。
`macos_update_coordinator.m`: 更新状态机宿主，转发交互式退出；若后台更新先替换官方包，则在宿主日后退出时调用同一固定 Helper 静默恢复且不擅自重开应用。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
