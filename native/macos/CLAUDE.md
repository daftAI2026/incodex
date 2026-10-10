# native/macos/
> L2 | 父级: ../CLAUDE.md

成员清单
permission-host-bridge.m: Objective-C，权限 host 的进程内原生桥接。
permission-host-flight.swift: Swift，权限引导飞行动画与窗口交接。
permission-host-objc.js: JavaScript，原生 Objective-C 桥接入口。
permission-host-osa.swift: Swift，权限页脚本交互适配。
permission-host-presenter.swift: Swift，权限卡呈现与生命周期。
permission-host-runtime.js: JavaScript，权限 host 的运行入口。
permission-host-settings.swift: Swift，系统设置窗口定位与引导。
permission-host.swift: Swift，独立原生权限 host 入口。
permission-views.swift: Swift，权限 UI dylib 视图与固定 ABI。
remote-key-compat.m: Objective-C/N-API，仅为验证的官方模块与 ad-hoc 宿主接入密钥兼容。
remote-key-policy.h: Security 属性白名单；只重试指定 P-256 请求且保持不可导出。
remote-key-compat.test.m: 临时私有 Keychain 回归；验证不可导出、签名与验签。
remote-key-error.test.m: 合成错误回归；验证重试成功后的错误所有权。
remote-key-host.test.m: 合成签名与私有临时登记回归；验证宿主身份门，不读取真实身份或 Keychain。
dist/: 已提交的双架构资产与源码/产物摘要；Rust publisher 验证后嵌入。
vendor/: 固定的 fishhook 与 Node N-API 头文件；保留第三方源码和许可。

原生 remote-key 入口不改变实验固定获权 helper。
本机证书签名不满足该入口的 ad-hoc 门。
该拒绝不阻塞官方启动。

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
