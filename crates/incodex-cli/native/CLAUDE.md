# crates/incodex-cli/native/
> L2 | 父级: ../CLAUDE.md

成员清单
fishhook.c: C，Mach-O 符号重绑定实现；供 Keychain provider 绑定系统函数。
fishhook.h: C header，声明符号重绑定入口与绑定记录。
macos_keychain_helper.m: Objective-C，隔离 Keychain helper；校验宿主、父进程与请求协议。
macos_keychain_provider.c: C，Keychain provider；在既有进程中受限重绑定并委托 helper。
macos_signing_store.m: Objective-C，私有签名身份 Keychain 的创建、只读验证与显式解锁。
macos_sparkle_interpose.m: Objective-C，匹配官方宿主与 Sparkle updater 初始化器，替换更新重开目标。
macos_update_coordinator.m: Objective-C，协调官方退出、更新交接与登记 CLI 恢复事务。

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
