# runtime/
> L2 | 父级: ../../CLAUDE.md

## 成员清单
accessibility-native-bundle.test.ts: 验证权限 UI 的封装、哈希与共享 Runtime 加载，不将宿主协议变成 renderer 能力。
accessibility-setup.test.ts: 权限引导编排回归，验证受控目标、主进程请求及窗口生命周期。
incodex-accessibility-native.cts: 原生权限引导呈现 adapter，消费已验证原生库与共享文案，权限决策留在宿主。
incodex-accessibility-native.test.ts: 原生引导呈现回归，覆盖展示门、状态和生命周期，不访问真实权限设置。
incodex-locale.cts: 共享语言选择器，供 renderer 与原生权限文案统一消费。
incodex-permission-card.cts: 权限卡片原生背景绘制，材料、阴影与轮廓保持已取证的官方视觉角色。
incodex-permission-card.test.ts: 权限卡片材料与深浅主题背景回归。
incodex-permission-graphics.cts: 原生绘制和对象所有权桥接，为权限卡片与过渡提供共享几何。
incodex-permission-graphics.test.ts: 绘制对象与缺失原生能力回归，验证桥接边界。
incodex-permission-motion.cts: 权限交接的共享弹簧采样与帧对齐，不承担权限变更。
incodex-permission-motion.test.ts: 交接运动时序、几何采样和帧对齐回归。
incodex-permission-native-motion.cts: 原生权限交接编排，复用共享运动与绘制能力管理复制视图。
incodex-permission-native-motion.test.ts: 原生复制视图和交接生命周期回归。
incodex-permission-native.cts: 原生权限库验证与加载，消费封装资产而非 renderer 提供的路径。
incodex-permission-native.test.ts: 原生库完整性、路径与加载安全回归。
incodex-permission-placeholder.cts: 权限占位状态绘制，区分正常、hover 和 pressed 表现。
incodex-permission-placeholder.test.ts: 占位交互状态和轮廓回归。
incodex-permission-ui.cts: 构建器封装的权限 UI 入口，旧 Electron 入口与 CLI 宿主共享同一产物。
incognito-accessibility-copy-data.ts: 权限引导区域语言文案，基础中英文仍由 incognito-copy.ts 管理。
incognito-accessibility-copy-runs.ts: 逐语言编写的拖拽指令语义片段，不从译文推断主次强调。
incognito-accessibility-official-copy-data.ts: 恢复官方 App 权限的专用文案，与安装理由隔离。
official-notifications.test.ts: 官方通知组件发现、呈现与销毁回归，验证隐私横幅和错误提示共源。
official-notifications.ts: 复用当前官方 React/通知组件，协调隐私横幅与启动错误，不维护第二套通知样式。
official-style-attributes.test.ts: CSS 声明驱动样式属性发现与更新回归，拒绝克隆身份和瞬时交互态。
official-style-attributes.ts: 从已加载官方 CSS 发现可继承 data 属性，同步 Search 外观，不写固定 Button 属性白名单。
permission-host-bundle.test.ts: CLI 权限宿主与 Electron 权限入口的封装资产、文案和哈希合同。
settings-locator.test.ts: 原生系统设置定位回归，绑定受验证 App 与 Settings 身份。
verified-main-test-fixture.ts: 生成哈希已验证的主进程测试资产，供 Runtime 安全加载测试复用。
button-icon-layout.ts: 克隆官方 SVG 的布局祖先，只保留样式属性，帽子与相机共享尺寸约束。
official-tooltip-renderer.ts: 从当前官方包加载 Tooltip/React 模块，持有提示根与跨注入共享状态，不引入第二个 React。
official-tooltip-renderer.test.ts: 官方提示模块发现、挂载销毁与重复注入状态共享回归。
inject-icon-layout.test.ts: Search 图标祖先布局、动态样式继承、属性剥离及帽子/相机图标切换回归。
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
inject.ts: 官方工具组前缘的帽子或实验相机集成；复用 #206 的动态样式属性、官方 Tooltip 与通知，Shot 外壳取样仍独立。
official-tooltip-provider.test.ts: 回归验证：发现官方 React provider 并复用提示时序。
official-tooltip-provider.ts: 发现官方 React provider 并复用提示时序。
search-button-placement.test.ts: 回归验证：Search 触发器边界与注入按钮位置。
search-button-placement.ts: Search 触发器边界与注入按钮位置。
status-menu.test.ts: 回归验证：状态栏菜单合同。
tooltip-lifecycle.test.ts: 回归验证：hover、焦点、关闭与延迟状态机。
tooltip-lifecycle.ts: hover、焦点、关闭与延迟状态机。
tooltip-presentation.test.ts: 回归验证：官方 tooltip 关联取样、缓存失效与兼容标题格式化。
tooltip-presentation.ts: 官方 tooltip 关联取样、缓存失效与兼容标题格式化；注入器不再使用原生 title 回退。
ui-probe.test.ts: 回归验证：注入健康验收。
window-lifecycle.test.ts: 回归验证：窗口关闭生命周期合同。
windows-platform.test.ts: 回归验证：Windows Runtime 平台合同。

## 动态样式边界
相机 tooltip 优先复用官方 Tooltip 组件，模块不可用时才取样 Search 关联提示；编辑器外壳只取样官方 dialog，不互换角色。未取样或不支持的声明保留兼容映射；不宣称所有控件已自动适配。无样本时不恢复原生 title；相机不显示无痕快捷键，重复注入复用同一提示生命周期。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

## #206 合流边界
官方通知、Search 样式发现和 Tooltip 呈现以 #206 为基底；实验相机继续复用同一注入按钮工厂。此次仅同步代码，不改变相机布局和工作台 token 角色映射；两者留待真实界面核对。
