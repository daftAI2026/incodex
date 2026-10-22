# crates/incodex-cli/native/
> L2 | 父级: ../CLAUDE.md

成员清单
fishhook.c: C，Mach-O 符号重绑定实现；供 Keychain provider 绑定系统函数。
fishhook.h: C header，声明符号重绑定入口与绑定记录。
macos_keychain_helper.m: Objective-C，隔离 Keychain helper；校验宿主、父进程与请求协议。
macos_launcher_icon.m: Objective-C，读取显式.app图标；叠加透明铭牌并输出16至1024像素ICNS。
macos_launcher_icon_orientation_test.m: Objective-C，临时包含生产合成器；PNG往返后断言非对称源图与徽标方向。
macos_keychain_provider.c: C，Keychain provider；在既有进程中受限重绑定并委托 helper。
macos_signing_store.m: Objective-C，私有签名身份 Keychain 的创建、只读验证与显式解锁。
macos_sparkle_interpose.m: Objective-C，精确替换updater重开目标；原回调正常返回后在原pending锁内标记排队。
macos_update_pending.h: Objective-C header，共用私有JSON、异常必释放的锁和排队标记；不增加安装授权。
macos_update_diagnostics.h: Objective-C/C 共用，精确环境门与闭集 update 事件；产品用 os_log，测试 sink 用 stderr。
macos_update_coordinator.m: Objective-C，协调官方退出与登记CLI；新宿主抢先启动时保留owner，以退出事件续接且不重开。

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
