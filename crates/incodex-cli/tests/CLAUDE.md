# tests/
> L2 | 父级: ../CLAUDE.md

## 成员清单

该目录承载原生 CLI 的黑盒、跨进程和平台契约测试；文件名即对应被验证的产品边界。`macos_update_restore.rs` 专门约束 Runtime、Coordinator、Sparkle interposer、handoff 所有权与内容寻址 Helper 的跨代恢复状态机，其余测试保持各自命名所示的安装、诊断、会话、更新及 Windows 边界。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
