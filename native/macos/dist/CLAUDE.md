# native/macos/dist/
> L2 | 父级: ../CLAUDE.md

成员清单
incodex-permission-host: 双架构权限卡；原版拖放只发收据，安装保留真实探针，不执行 TCC 动作。
incodex-permission-ui.dylib: 双架构共用视图与系统呈现 ABI。
incodex-remote-key-compat.node: 双架构官方密钥接口的窄范围适配。
runtime-native-manifest.json: 绑定权限源顺序、ABI 与二进制摘要。
incodex-remote-key-manifest.json: 绑定密钥适配源与二进制摘要。

安装与 Runtime 构建只读取已验证资产。
修改源后，重建资产并更新摘要。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
