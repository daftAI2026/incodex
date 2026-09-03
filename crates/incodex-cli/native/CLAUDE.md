# native/
> L2 | 父级: ../CLAUDE.md

## 成员清单

`macos_sparkle_interpose.m`: Sparkle 初始化窄缝 interposer，仅把更新生命周期的 application bundle 交给外置 Coordinator。
`macos_update_coordinator.m`: 更新状态机宿主，以 handoff 所有权约束 pending 清理，并在执行前校验当前注册的内容寻址 Helper；后台换包后静默恢复且不擅自重开应用。
`macos_keychain_helper.m`: 固定身份的 Keychain 读取进程，区分显式前台授权与无交互运行查询，并校验宿主调用者身份。
`macos_keychain_provider.c`: 仅拦截 Codex Storage Key 的精确查询，通过固定 helper 有界读取，任何增强路径失败都回退官方 Security 语义。
`fishhook.c`: BSD-3-Clause Mach-O 符号重绑定实现，供 Keychain provider 接管单一 Security 入口。
`fishhook.h`: fishhook 的最小公开声明，只暴露 provider 安装重绑定所需接口。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
