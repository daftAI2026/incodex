# native/macos/
> L2 | 父级: ../CLAUDE.md

成员清单
permission-host-bridge.m: Objective-C，权限 host 的进程内原生桥接。
permission-host-flight.swift: Swift，权限引导飞行动画与窗口交接。
permission-host-objc.js: JavaScript，原生 Objective-C 桥接入口。
permission-host-osa.swift: Swift，权限页脚本交互适配。
permission-host-presenter.swift: Swift，权限卡状态与生命周期；退回时统一停表、退役 flight，并保留进行中的拖拽 source。
permission-host-presenter-geometry.swift: Swift，Presenter 尺寸约束、屏幕坐标转换与视图图片缓存。
permission-host-presenter-views.swift: AppKit 图标、action 与 helper/arrow/drag 适配；UTType 缺失回退符号，drag source 保留至 ended；双行已有 helper 暂失只等待，legacy 单行十次 miss 发 Later。
permission-host-runtime.js: JavaScript，权限 host 的运行入口。
permission-host-settings.swift: Swift，系统设置窗口定位与引导。
permission-host.swift: Swift，权限 stdio 入口；仅 installation 双行上下文免工作窗门，默认呈现门不变。
permission-views.swift: Swift，共用系统呈现适配、快照与 helper ABI；26 保留旧文案，27 读取当前系统词表和权限图标。
permission-initial.swift: Swift，独立权限行状态与双行视图；既有单卡适配保持原 Objective-C ABI。
host-identity.h: Objective-C，原生只读宿主身份接口；复用既有公开证书登记，不读取私钥或推断授权。
host-identity-real.test.m: Security 实签回归；诊断仅查隔离身份的输入形状与keyUsage，不调用AX或远控密钥。
remote-key-compat.m: Objective-C/N-API，复用宿主身份接口，仅为验证的官方模块接入密钥兼容。
remote-key-policy.h: Security 属性白名单；只重试指定 P-256 请求且保持不可导出。
remote-key-compat.test.m: 临时私有 Keychain 回归；验证不可导出、签名与验签。
remote-key-error.test.m: 合成错误回归；验证重试成功后的错误所有权。
remote-key-host.test.m: 合成签名与私有临时登记回归；验证宿主身份门，不读取真实身份或 Keychain。
dist/: 已提交的双架构资产与源码/产物摘要；Rust publisher 验证后嵌入。
vendor/: 固定的 fishhook 与 Node N-API 头文件；保留第三方源码和许可。

原生身份接口保留 ad-hoc，并验证登记证书宿主。
证书登记与实际叶证书、严格签名验证必须一致。
远控入口不改变实验固定获权 helper。
登记损坏拒绝本机证书路径，不阻塞官方启动。
公开证书验证不等于签名 Keychain 健康检查。

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
