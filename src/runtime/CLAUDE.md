# src/runtime/
> L2 | 父级: ../CLAUDE.md

成员清单
accessibility-native-bundle.test.ts: TypeScript，辅助功能权限证据与引导；回归边界
accessibility-setup.test.ts: TypeScript，辅助功能权限证据与引导；回归边界
button-icon-layout.ts: TypeScript，继承 Search 图标的布局祖先，不复制交互
codex-mode-readiness.test.ts: TypeScript，Codex 路由就绪和有界 fallback；回归边界
dock-menu.test.ts: TypeScript，原生菜单交互；回归边界
electron.d.ts: TypeScript，Electron API 编译期声明
experiment-integration.test.ts: 合流与退出诊断 opt-in 接线回归；不访问真实 App 或用户状态
incodex-accessibility-native.cts: Electron TypeScript，兼容原生 AX 引导；名称与图标复用 Swift 系统适配
incodex-accessibility-native.test.ts: TypeScript，辅助功能权限证据与引导；回归边界
incodex-codex-mode.cts: Electron TypeScript，Codex 模式探测、官方阻塞及异步 fallback
incodex-dock-menu.cts: Electron TypeScript，原生菜单交互
incodex-instance.cts: Electron TypeScript，隔离实例 PID、raise 通道与 owner 租约
incodex-ipc-guard.cts: Electron TypeScript，跨进程请求守卫
incodex-loader.cts: Electron TypeScript，ASAR 的校验 bootstrap；保留官方同步初始化和 native-open 单 owner 边界
incodex-locale.cts: Electron TypeScript，语言策略
incodex-macos-update.cts: Electron TypeScript，验证更新桥接与同代 helper；权限任务绑定原 requestId
incodex-main.cts: Electron 主进程编排层；宿主事件及单 watcher 唤醒三类续接 marker，原生 owner 复验
incodex-owner-core.cts: Electron TypeScript，跨平台 owner 证据与回收判定
incodex-owner-recovery.cts: Electron TypeScript，不确定所有权状态的安全恢复
incodex-permission-card.cts: Electron TypeScript，原生权限呈现
incodex-permission-card.test.ts: TypeScript，原生权限呈现；回归边界
incodex-permission-graphics.cts: Electron TypeScript，原生权限呈现
incodex-permission-graphics.test.ts: TypeScript，原生权限呈现；回归边界
incodex-permission-motion.cts: Electron TypeScript，原生权限呈现
incodex-permission-motion.test.ts: TypeScript，原生权限呈现；回归边界
incodex-permission-native-motion.cts: Electron TypeScript，原生权限呈现
incodex-permission-native-motion.test.ts: TypeScript，原生权限呈现；回归边界
incodex-permission-native.cts: Electron TypeScript，原生权限呈现
incodex-permission-native.test.ts: TypeScript，原生权限呈现；回归边界
incodex-permission-placeholder.cts: Electron TypeScript，原生权限呈现
incodex-permission-placeholder.test.ts: TypeScript，原生权限呈现；回归边界
incodex-permission-ui.cts: Electron TypeScript，原生权限呈现
incodex-preload.cts: Electron 顶层 frame 的窄 IPC 桥；只转发动作，来源与权限校验留在 main
incodex-runtime-load.cts: 固定信任边界与代际提交；权限业务复用已部署协议，不替换验证器
incodex-safe-home.cts: Electron TypeScript，隔离 home 安全边界
incodex-ui-probe.ts: TypeScript，官方 UI 语义探测
incodex-window-kind.cts: Electron TypeScript，窗口归属与显示
incodex-window-lifecycle.cts: Electron TypeScript，窗口归属与显示
incognito-accessibility-copy-data.ts: TypeScript，纯组合地区权限引导；A-K 与 L-Z 共用入口，renderer 不嵌入权限文案
incognito-accessibility-copy-data-extra.ts: TypeScript，L-Z 权限引导字面量；不增加 Runtime 资产
incognito-accessibility-copy-runs.ts: TypeScript，辅助功能权限证据与引导
incognito-accessibility-official-copy-data.ts: TypeScript，65 语言官方身份权限说明；与安装原因分离
incognito-copy-data.ts: TypeScript，用户可见文案
incognito-copy.ts: TypeScript，65 语言原始 AX 文案与私密窗口源文案；权限小卡统一投影由 src/permission-shared-copy.ts 生成
incognito-profile-rail.test.ts: TypeScript，真实 selector/VM 验证头像型 rail 与重复 bootstrap；不读取真实身份
incognito-profile-mask.ts: TypeScript，遮罩唯一 sidebar 身份与关联菜单；支持头像型 rail，未知结构和歧义拒绝
inject-icon-layout.test.ts: renderer 图标继承回归；共用同一 DOM fixture 避免跨 document 误判
inject.test.ts: TypeScript，隔离 DOM/VM 验证身份遮罩、按钮重挂载与呈现生命周期
inject.ts: TypeScript，共享隐私 UI 注入入口；重复 bootstrap 复用侧栏根和 activation callback
official-notifications.test.ts: TypeScript，官方通知组件识别、呈现与关闭调和；回归边界
official-notifications.ts: TypeScript，官方通知组件识别、呈现与关闭调和
official-sidebar.test.ts: TypeScript，合成 DOM/Fiber 验证侧栏归属、属性继承及歧义拒绝
official-sidebar.ts: TypeScript，只读发现官方导航栏组件与插入位置，不读取账号或聊天子树
official-sidebar-entry.test.ts: TypeScript，合成渲染器验证入口挂载、异步竞争与销毁
official-sidebar-entry.ts: TypeScript，复用发现层的官方组件渲染入口，不冻结宿主样式
official-style-attributes.test.ts: TypeScript，从官方样式声明发现 style token；回归边界
official-style-attributes.ts: TypeScript，从官方样式声明发现 style token
official-tooltip-provider.test.ts: TypeScript，官方 Tooltip 识别与生命周期；回归边界
official-tooltip-provider.ts: TypeScript，官方 Tooltip 识别与生命周期
official-tooltip-renderer.test.ts: TypeScript，官方 Tooltip 识别与生命周期；回归边界
official-tooltip-renderer.ts: TypeScript，官方 Tooltip 识别与生命周期
permission-probe.test.ts: 原子 UUID 请求、身份与真实 watcher 回归；不访问真实权限
quit-trace.test.ts: TypeScript，合成 Module 与 Electron 事件夹具；验证官方 getter 与退出事件同步/派发后只读边界
incodex-permission-probe.cts: 主进程只读录屏 responder；复用既有 UUID 协议，保留 nonce 与真实读数
permission-host-bundle.test.ts: TypeScript，原生权限呈现；回归边界
search-button-placement.test.ts: TypeScript，定位 Search 控件并维持相邻布局；回归边界
search-button-placement.ts: TypeScript，定位 Search 控件并维持相邻布局
settings-locator.test.ts: TypeScript，设置导航与加载骨架识别；回归边界
status-menu.test.ts: TypeScript，原生菜单交互；回归边界
tooltip-lifecycle.test.ts: TypeScript，官方 Tooltip 识别与生命周期；回归边界
tooltip-lifecycle.ts: TypeScript，官方 Tooltip 识别与生命周期
tooltip-presentation.test.ts: TypeScript，官方 Tooltip 识别与生命周期；回归边界
tooltip-presentation.ts: TypeScript，官方 Tooltip 识别与生命周期
ui-probe.test.ts: TypeScript，官方 UI 语义探测；回归边界
verified-main-test-fixture.ts: TypeScript，临时已验证 Runtime 发布夹具
window-lifecycle.test.ts: TypeScript，窗口归属与显示；回归边界
windows-platform.test.ts: TypeScript，窗口归属与显示；回归边界

incodex-main-actions.cts: protocol 1 纯动作工厂；可选权限呈现规则，只读代际文案，不持有授权事实
incodex-remote-key-compat.cts: macOS 可选原生入口，校验同代 addon 后调用官方 secure-key 模块；失败放行官方启动
incodex-quit-trace.cts: Electron 主进程 opt-in 诊断；记录官方 getter bool 与生命周期同步/派发后状态，不接管退出
remote-key-compat.test.ts: 合成门与隔离实签回归；复用实验签名后端，验证身份、不可导出与错误所有权。
renderer-generation.ts: renderer 代际切换与回滚协议，ACK 必须对应当前注入器身份
renderer-generation.test.ts: 合成 DOM 回归，验证代际切换、旧回调退休和重复 bootstrap

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
