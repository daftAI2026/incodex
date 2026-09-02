# native/
> L2 | 父级: ../CLAUDE.md

## 成员清单

`macos_sparkle_interpose.m`: Sparkle 初始化窄缝 interposer，仅把更新生命周期的 application bundle 交给外置 Coordinator。
`macos_update_coordinator.m`: 更新状态机宿主，转发交互式退出，并在官方包替换后调用固定 Helper 恢复 Incodex。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
