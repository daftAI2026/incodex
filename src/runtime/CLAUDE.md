# src/runtime/
> L2 | 父级: ../CLAUDE.md

成员清单
accessibility-native-bundle.test.ts: TypeScript，辅助功能权限证据与引导；回归边界
accessibility-setup.test.ts: TypeScript，辅助功能权限证据与引导；回归边界
button-icon-layout.ts: TypeScript，继承 Search 图标的布局祖先，不复制交互
codex-mode-readiness.test.ts: TypeScript，Codex 路由就绪和有界 fallback；回归边界
dock-menu.test.ts: TypeScript，原生菜单交互；回归边界
electron.d.ts: TypeScript，Electron API 编译期声明
experiment-integration.test.ts: 实验合流回归边界，不访问真实 App、Keychain 或用户 Runtime
incodex-accessibility-native.cts: Electron TypeScript，辅助功能权限证据与引导
incodex-accessibility-native.test.ts: TypeScript，辅助功能权限证据与引导；回归边界
incodex-codex-mode.cts: Electron TypeScript，Codex 模式探测、官方阻塞及异步 fallback
incodex-dock-menu.cts: Electron TypeScript，原生菜单交互
incodex-instance.cts: Electron TypeScript，隔离实例 PID、raise 通道与 owner 租约
incodex-ipc-guard.cts: Electron TypeScript，跨进程请求守卫
incodex-loader.cts: Electron TypeScript，ASAR 到外部 Runtime 的加载
incodex-locale.cts: Electron TypeScript，语言策略
incodex-macos-update.cts: Electron TypeScript，更新代际与恢复
incodex-main.cts: Electron 主进程编排层；不阻塞官方初始化，owner 确定后才展示实验无痕窗口
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
incodex-preload.cts: Electron TypeScript，受限 renderer IPC bridge
incodex-runtime-load.cts: Electron TypeScript，外部 Runtime 构建与验证
incodex-safe-home.cts: Electron TypeScript，隔离 home 安全边界
incodex-ui-probe.ts: TypeScript，官方 UI 语义探测
incodex-window-kind.cts: Electron TypeScript，窗口归属与显示
incodex-window-lifecycle.cts: Electron TypeScript，窗口归属与显示
incognito-accessibility-copy-data.ts: TypeScript，辅助功能权限证据与引导
incognito-accessibility-copy-runs.ts: TypeScript，辅助功能权限证据与引导
incognito-accessibility-official-copy-data.ts: TypeScript，辅助功能权限证据与引导
incognito-copy-data.ts: TypeScript，用户可见文案
incognito-copy.ts: TypeScript，用户可见文案
incognito-profile-mask.ts: TypeScript，仅掩码 sidebar/account identity
inject-icon-layout.test.ts: renderer 图标继承回归；共用同一 DOM fixture 避免跨 document 误判
inject.test.ts: TypeScript，共享 renderer 注入；回归边界
inject.ts: TypeScript，共享 renderer 注入
official-notifications.test.ts: TypeScript，官方通知组件识别、呈现与关闭调和；回归边界
official-notifications.ts: TypeScript，官方通知组件识别、呈现与关闭调和
official-style-attributes.test.ts: TypeScript，从官方样式声明发现 style token；回归边界
official-style-attributes.ts: TypeScript，从官方样式声明发现 style token
official-tooltip-provider.test.ts: TypeScript，官方 Tooltip 识别与生命周期；回归边界
official-tooltip-provider.ts: TypeScript，官方 Tooltip 识别与生命周期
official-tooltip-renderer.test.ts: TypeScript，官方 Tooltip 识别与生命周期；回归边界
official-tooltip-renderer.ts: TypeScript，官方 Tooltip 识别与生命周期
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

法则: 成员完整·一行一文件·父级链接·技术词前置
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
