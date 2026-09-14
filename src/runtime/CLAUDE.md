# runtime/
> L2 | 父级: [../CLAUDE.md](../CLAUDE.md)

本层是 Electron-side Runtime 的唯一实现面：主进程负责隔离会话、所有权、窗口与 IPC；浏览器注入负责官方页面上的隐私控件、文案、头像遮罩、Search 布局和 tooltip；测试以官方 DOM/生命周期契约约束两条数据流。`incodex-loader.cts` 是官方 asar 内唯一 loader，其余 Runtime 由清单验证后加载。

## 成员清单
- `codex-mode-readiness.test.ts`: 验证 Codex 模式探测、官方阻塞层识别与延迟重试调度。
- `compatibility/search-labels.ts`: 集中维护官方 Search aria-label 多语言集合与前缀识别，供注入层发现 Search 按钮。
- `dock-menu.test.ts`: 验证 macOS Dock 菜单装饰的插入、去重、标签清洗与异常降级。
- `electron.d.ts`: 为 Runtime 编译提供最小 Electron 模块类型声明，不承载运行时逻辑。
- `incodex-codex-mode.cts`: 在渲染器中探测官方模式菜单与阻塞层，并以可重试 readiness 状态驱动 Codex 模式回退。
- `incodex-dock-menu.cts`: 管理 macOS Dock 与状态菜单的 Incodex 项、原生桥接和菜单变更监听，失败不干扰官方菜单。
- `incodex-instance.cts`: 基于目标可执行文件、进程身份、锁文件与本地端口实现主/私密实例租约、唤回和 helper 清理。
- `incodex-ipc-guard.cts`: 将 Electron IPC 请求绑定到可信 origin、主 Frame、窗口及 webContents 身份，拒绝导航后身份漂移。
- `incodex-loader.cts`: 在官方 asar 内校验当前 Runtime manifest、哈希、路径与签名边界，失败开放回官方主入口。
- `incodex-main.cts`: Electron 主进程编排器，连接安全 home、实例租约、窗口分类/生命周期、IPC、平台桥接与私密窗口启动。
- `incodex-owner-core.cts`: 定义目标状态目录、所有者 token、进程启动身份、锁/记录路径及可验证的 owner 基础不变量。
- `incodex-owner-recovery.cts`: 承担 owner 租约抢占、诊断隔离、本地 socket 协议、端口探测与恢复，依赖 core 而不反向耦合主进程。
- `incodex-preload.cts`: 只向顶层可信 Frame 暴露最小 `incodex-action` IPC bridge，隔离 iframe 与页面脚本。
- `incodex-runtime-load.cts`: 解析生产内置 Runtime 与 `INCODEX_DEV_HOT=1` 开发覆盖路径，按可执行文件目标隔离热加载资产。
- `incodex-safe-home.cts`: 创建、复制、交接、烧毁并回收身份绑定的私密会话目录，拒绝 symlink/越界并保留烧毁证明与日志轮转。
- `incodex-ui-probe.ts`: 将注入按钮、banner、tooltip 与私密态压缩为稳定的 Runtime UI 健康快照。
- `incodex-window-kind.cts`: 依据窗口尺寸、可聚焦性、置顶状态与 URL 区分主/登录对话框和辅助窗口。
- `incodex-window-lifecycle.cts`: 封装私密窗口关闭、隐藏、恢复与延迟烧毁的生命周期状态机。
- `incognito-copy-data.ts`: 保存各语言的私密窗口标题、按钮、banner、错误与关闭文案数据表。
- `incognito-copy.ts`: 对文案数据做 locale 解析与稳定回退，向注入层提供统一翻译接口。
- `incognito-profile-mask.ts`: 只在侧栏身份及其一级账户菜单内发现、遮罩并健康检查用户名称/头像，未知结构 fail-closed。
- `inject.test.ts`: 以源码契约和资产摘要检查注入、身份遮罩、banner、tooltip 与控件重挂载约定。
- `inject.ts`: 浏览器 Runtime 注入编排器，发现官方 Search 与 tooltip，挂载帽子/退出控件、banner、快捷键及私密身份遮罩，并向主进程请求动作。
- `official-tooltip-provider.test.ts`: 验证从官方 React Fiber context 按能力发现 tooltip provider，而非依赖固定组件路径。
- `official-tooltip-provider.ts`: 读取官方 tooltip provider 的延迟与打开/关闭能力，为自有控件复用官方交互时序。
- `search-button-placement.test.ts`: 验证 Search 处于 tooltip wrapper 时插入点位于 wrapper 之前，否则保持直接兄弟布局。
- `search-button-placement.ts`: 只计算 Search 旁的安全挂载父节点/前置锚点及 tooltip 打开状态，不负责创建控件。
- `status-menu.test.ts`: 验证状态菜单中 Incodex 项的识别、插入、去重与 observer 释放。
- `tooltip-lifecycle.test.ts`: 验证 pointer/focus/window 状态、延迟展示、取消与 dismiss 的状态机。
- `tooltip-lifecycle.ts`: 以可注入调度器实现 tooltip 的展示生命周期，隔离计时器、官方 provider 与 DOM 展示。
- `tooltip-presentation.test.ts`: 验证从当前官方 tooltip 读取语义 class/token、窗口缩放与动态主题变化。
- `tooltip-presentation.ts`: 通过 Search trigger 的 `aria-describedby` 找到官方 tooltip，提取当前 class 与缩放 token，禁止猜测页面样式。
- `ui-probe.test.ts`: 验证 Runtime UI 健康快照对普通窗口、私密 banner 缺失/关闭与按钮缺失的判定。
- `window-lifecycle.test.ts`: 使用最小窗口夹具验证私密窗口 close/closed/show 生命周期与退出回调。
- `windows-platform.test.ts`: 验证 Windows guardian ready/closed pipe、异步 UI readiness 与不暴露官方退出能力。
- `inject-icon-layout.test.ts`: 通过隔离 DOM 夹具执行实际注入函数，验证官方图标布局与 token 引用不丢失。

法则: 成员完整·一行一文件·主进程与渲染器依赖方向分离·官方 token 优先·失败开放且安全。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
