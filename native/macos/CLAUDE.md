# native/macos/
> L2 | 父级: ../CLAUDE.md

成员清单
permission-host-bridge.m: Objective-C，权限 host 的进程内原生桥接。
permission-host-flight.swift: Swift，权限引导飞行动画与窗口交接。
permission-host-objc.js: JavaScript，原生 Objective-C 桥接入口。
permission-host-osa.swift: Swift，权限页脚本交互适配。
permission-host-presenter.swift: Swift，权限卡状态与生命周期；手动 Screen 收据只回卡，不授予权限。
permission-host-presenter-geometry.swift: Swift，Presenter 尺寸、AppKit/Quartz 拖放坐标转换与图片缓存。
permission-host-presenter-views.swift: AppKit action 与拖放适配；仅精确 copy 命中前台 Settings 窗才发一次收据。
permission-host-runtime.js: JavaScript，权限 host 的运行入口。
permission-host-settings.swift: Swift，系统设置窗口定位与引导。
permission-host.swift: Swift，权限 stdio 入口；校验 probe/manual 配置和 manual-submitted 状态。
permission-views.swift: Swift，共用系统呈现适配、快照与 helper ABI；26 保留旧文案，27 读取当前系统词表和权限图标。
permission-initial.swift: Swift，独立权限行状态与双行视图；manual Screen 保持未知禁用，AX 独立可操作。
host-identity.h: Objective-C，原生只读宿主身份接口；复用既有公开证书登记，不读取私钥或推断授权。
host-identity-real.test.m: Security 实签回归；诊断以codesign实际SecPolicyCopy构造查询，不调用AX或远控密钥。
remote-key-compat.m: Objective-C/N-API，保留旧 create 回退；不承载热 adapter bridge。
device-key-adapter.m: Objective-C/N-API，独立热 adapter；预检本机宿主、官方模块和固定 helper，不装 fishhook。
remote-key-policy.h: Security 属性白名单；只重试指定 P-256 请求且保持不可导出。
remote-key-compat.test.m: 临时私有 Keychain 回归；验证不可导出、签名与验签。
remote-key-error.test.m: 合成错误回归；验证重试成功后的错误所有权。
remote-key-host.test.m: 旧 remote-key addon 宿主身份回归；新 adapter 宿主门由独立 fixture 验证。
device-key-bridge.h: N-API 边界；验证固定 helper 与有界 stdio；正式期限 300 秒，夹具可缩短；不持有 Keychain 句柄。
device-key-bridge.test.m: 合成 transport 回归；验证身份、长度、期限与损坏拒绝。
device-key-bridge-napi.test.m: N-API fixture；按操作门闩；验证 Node 池、期限后签名、Worker 取消与拒绝码；不访问真实钥匙串。
device-key-adapter-host.test.m: 合成签名回归；验证新 adapter 不依赖旧 fishhook 状态，不访问 Keychain。
device-key-consumer.h: 闭域 device-key 消费接口；只读公钥或签 allowlist payload，不创建或导出密钥。
device-key-consumer.test.m: 临时 Keychain 回归；验证现有 P-256 身份、闭域签名与拒绝路径。
device-key-helper.m: 短命固定消费者；验证精确宿主，只查询现存 OSN 私钥，不改 ACL 或 Storage。
device-key-helper-lifecycle.test.m: 合成父进程回归；验证精确宿主退出后 helper 停止。
dist/: 已提交的双架构资产与源码/产物摘要；Rust publisher 验证后嵌入。
vendor/: 固定的 fishhook 与 Node N-API 头文件；保留第三方源码和许可。

原生身份接口保留 ad-hoc，并验证登记证书宿主。
证书登记与实际叶证书、严格签名验证必须一致。
远控入口不改变实验固定获权 helper。
登记损坏拒绝本机证书路径，不阻塞官方启动。
公开证书验证不等于签名 Keychain 健康检查。
固定 device helper 与 Storage helper 分离；helper 登记不等于 Keychain 授权通过。
拒绝或取消保持原错误，不回退到官方签名调用。

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
