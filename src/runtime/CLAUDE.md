# runtime/
> L2 | 父级: ../CLAUDE.md

## 成员清单

CLAUDE.md: runtime 局部地图，维护 Electron 扩展面的职责与依赖方向。
codex-mode-readiness.test.ts: 无痕窗口 Codex 模式探测与有限回退测试。
compatibility/: 历史 Runtime 兼容夹具，限制旧磁盘状态的读取边界。
dock-menu.test.ts: macOS Dock 与状态菜单控制器测试。
electron.d.ts: Runtime 编译所需的最小 Electron 类型声明。
incodex-codex-mode.cts: 无痕窗口模式就绪状态机，只在确认非 Codex 模式后触发有限回退。
incodex-dock-menu.cts: macOS 原生 Dock/状态菜单桥接器，将入口统一路由到无痕启动动作。
incodex-instance.cts: Runtime 实例身份、锁、端口与进程所有权原语。
incodex-ipc-guard.cts: 渲染器来源和窗口身份校验，隔离非授权 IPC。
incodex-loader.cts: 官方 ASAR 内唯一加载器，校验外置 Runtime 后 fail-open 到官方 main。
incodex-macos-update.cts: Sparkle 自动恢复交接边界，在不让出事件循环的前提下同步布防 Coordinator 与 interposer。
incodex-main.cts: Electron Runtime 主编排器，挂接官方窗口、会话生命周期、菜单与平台适配器。
incodex-owner-core.cts: 会话 owner 身份模型与跨进程存活证明。
incodex-owner-recovery.cts: 异常退出后的 owner 恢复与保守清理策略。
incodex-preload.cts: 官方 preload 外层桥接，暴露最小受控 Incodex 能力。
incodex-runtime-load.cts: 外置 Runtime 文件定位与加载辅助。
incodex-safe-home.cts: 隔离 CODEX_HOME 的创建、白名单复制、就绪和焚毁实现。
incodex-ui-probe.ts: 注入结果的结构化健康判定。
incodex-window-kind.cts: 主内容窗口与辅助窗口分类器。
incodex-window-lifecycle.cts: 无痕主窗口关闭状态机，等待官方 close 被接受后再清理。
incognito-copy-data.ts: 官方支持语言对应的 Incodex 文案数据。
incognito-copy.ts: Incodex 文案语言解析与回退策略。
incognito-profile-mask.ts: 截图隐私遮罩的 DOM 识别与骨架布局模型。
inject.test.ts: 共享渲染器注入器行为测试。
inject.ts: 帽子眼镜、提示、横幅和隐私遮罩的共享渲染器注入实现。
official-tooltip-provider.test.ts: 官方 tooltip 宿主复用测试。
official-tooltip-provider.ts: 查找并复用官方 tooltip provider 的适配器。
search-button-placement.test.ts: 帽子眼镜相对 Search 的稳定定位测试。
search-button-placement.ts: Search 同级插槽识别与放置策略。
status-menu.test.ts: 原生状态菜单配置与生命周期测试。
tooltip-lifecycle.test.ts: tooltip 打开、焦点与销毁状态机测试。
tooltip-lifecycle.ts: tooltip 生命周期状态机。
tooltip-presentation.test.ts: tooltip 几何与呈现策略测试。
tooltip-presentation.ts: tooltip 位置和视口约束计算。
ui-probe.test.ts: 注入健康探针测试。
window-lifecycle.test.ts: 无痕主窗口关闭接受语义测试。
windows-platform.test.ts: Windows Runtime 平台桥接与握手测试。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
