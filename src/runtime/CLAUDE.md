# runtime/
> L2 | 父级: ../../CLAUDE.md

## 成员清单
capture-window/: Shot 实验编辑器与测试，入口见 CLAUDE.md。
codex-mode-readiness.test.ts: 回归验证：官方模式就绪时序合同。
compatibility/: 官方控件兼容标识。
dock-menu.test.ts: 回归验证：Dock 菜单合同。
electron.d.ts: Electron 类型边界。
incodex-codex-mode.cts: 官方模式就绪探测。
incodex-dock-menu.cts: macOS Dock 与状态栏菜单集成。
incodex-instance.cts: 窗口实例协调。
incodex-ipc-guard.cts: 受信 IPC 请求校验。
incodex-loader.cts: 官方 asar 内的最小 fail-open bootstrap。
incodex-main.cts: Electron 主进程 Runtime 编排。
incodex-owner-core.cts: 会话 owner 身份校验。
incodex-owner-recovery.cts: 会话 owner 恢复。
incodex-preload.cts: 受控 renderer bridge。
incodex-runtime-load.cts: 外部 Runtime 哈希验证与加载。
incodex-safe-home.cts: 临时 home 创建与安全清理。
incodex-ui-probe.ts: 注入控件健康快照。
incodex-window-kind.cts: 窗口类型判断。
incodex-window-lifecycle.cts: 无痕窗口生命周期。
incognito-copy-data.ts: 多语言文案数据。
incognito-copy.ts: 语言选择与文案查找。
incognito-profile-mask.ts: 临时资料名称和头像遮罩。
inject.test.ts: 回归验证：官方 Search 旁的帽子或相机集成；协调 tooltip 与 Shot 外壳动态取样。
inject.ts: 官方 Search 旁的帽子或相机集成；协调 tooltip 与 Shot 外壳动态取样。
official-tooltip-provider.test.ts: 回归验证：发现官方 React provider 并复用提示时序。
official-tooltip-provider.ts: 发现官方 React provider 并复用提示时序。
search-button-placement.test.ts: 回归验证：Search 触发器边界与注入按钮位置。
search-button-placement.ts: Search 触发器边界与注入按钮位置。
status-menu.test.ts: 回归验证：状态栏菜单合同。
tooltip-lifecycle.test.ts: 回归验证：hover、焦点、关闭与延迟状态机。
tooltip-lifecycle.ts: hover、焦点、关闭与延迟状态机。
tooltip-presentation.test.ts: 回归验证：官方 tooltip 关联取样、缓存失效与无样本原生提示。
tooltip-presentation.ts: 官方 tooltip 关联取样、缓存失效与无样本原生提示。
ui-probe.test.ts: 回归验证：注入健康验收。
window-lifecycle.test.ts: 回归验证：窗口关闭生命周期合同。
windows-platform.test.ts: 回归验证：Windows Runtime 平台合同。

## 动态样式边界
相机 tooltip 只取样 Search 关联提示；编辑器外壳只取样官方 dialog，不互换角色。未取样或不支持的声明保留兼容映射；不宣称所有控件已自动适配。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
