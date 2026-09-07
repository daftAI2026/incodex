# capture-window/
> L2 | 父级: ../CLAUDE.md

## 成员清单
background-controls.ts: 背景选择事件与选中态同步，命令交给编辑器。
background-picker.css: 背景分组与色板几何，消费共享视觉 token。
backgrounds.test.ts: 回归验证：图片背景加载与缓存，供 compositor 使用。
backgrounds.ts: 图片背景加载与缓存，供 compositor 使用。
capture-lifecycle.test.ts: 回归验证：隐藏 UI、等待绘制与捕获恢复的事务。
capture-lifecycle.ts: 隐藏 UI、等待绘制与捕获恢复的事务。
capture-window.css: 编辑器布局与默认 token 映射，运行时样本可覆盖外壳语义。
cdp-bridge.test.ts: 回归验证：截图请求关联与超时，隔离 CDP 传输和编辑器。
cdp-bridge.ts: 截图请求关联与超时，隔离 CDP 传输和编辑器。
color-popover.css: 颜色浮层的交互或视觉契约。
color-popover.test.ts: 回归验证：颜色浮层的交互或视觉契约。
color-popover.ts: 颜色浮层的交互或视觉契约。
compositor.test.ts: 回归验证：全分辨率背景、padding 与隐私区域像素合成。
compositor.ts: 全分辨率背景、padding 与隐私区域像素合成。
copy.ts: 编辑器中英文文案。
editor.ts: 编辑命令、预览绘制和输入生命周期编排。
geometry.test.ts: 回归验证：contain fit、锚点缩放与区域坐标计算。
geometry.ts: contain fit、锚点缩放与区域坐标计算。
icons.ts: 共享 Lucide 风格 SVG 资源。
injected.ts: 截图准备、受控 host 挂载与 CDP adapter 集成。
live-tokens.test.ts: 回归验证：从官方 dialog 实际 utility 声明提取语义变量，拒绝固定 RGB 与局部 Tailwind 中间量。
live-tokens.ts: 从官方 dialog 实际 utility 声明提取语义变量，拒绝固定 RGB 与局部 Tailwind 中间量。
model.test.ts: 回归验证：截图状态与命令转移，偏好和 UI 派生的唯一状态源。
model.ts: 截图状态与命令转移，偏好和 UI 派生的唯一状态源。
preferences.test.ts: 回归验证：持久化用户偏好，隔离临时编辑状态。
preferences.ts: 持久化用户偏好，隔离临时编辑状态。
presets.test.ts: 回归验证：图片及函数渐变元数据，色板和导出共源。
presets.ts: 图片及函数渐变元数据，色板和导出共源。
preview-contract.test.ts: 回归验证：独立预览结构与共享实现一致性。
preview.html: 独立浏览器开发入口或页面，不构成官方实窗验收。
preview.ts: 独立浏览器开发入口或页面，不构成官方实窗验收。
privacy.test.ts: 回归验证：官方 DOM 隐私候选与捕获前占位。
privacy.ts: 官方 DOM 隐私候选与捕获前占位。
redactions.test.ts: 回归验证：隐私区域遮罩采样与渲染。
redactions.ts: 隐私区域遮罩采样与渲染。
regions.ts: 编辑器区域覆盖层与命中几何。
tokens.test.ts: 回归验证：视觉角色、布局密度与 token 复用回归。
view.ts: 从状态生成编辑器 DOM，不持有第二份业务状态。
system-wallpaper-bridge.ts: 系统壁纸 CDP 请求关联与超时，拒绝远程资源和任意路径。
system-wallpaper-bridge.test.ts: 目录与编辑图片传输的有界白名单回归。
system-wallpapers.ts: 本机壁纸目录装载、选择竞态与 UI 生命周期；背景选择仍交给编辑状态机。
system-wallpapers.test.ts: 装载状态、空目录、失败及过期异步结果的回归。
wallpaper.ts: 用户壁纸输入与图像处理边界。

## 动态样式边界
相机 tooltip 只取样 Search 关联提示；编辑器外壳只取样官方 dialog，不互换角色。未取样或不支持的声明保留兼容映射；不宣称所有控件已自动适配。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

## 2026-09-08 实窗取证

- 调试入口：本工作树构建 Runtime 后，`INCODEX_CAPTURE_DEBUG=1 cargo run -p incodex-cli -- open`；不是独立 preview，也不是普通主窗口接管验收。
- 当前官方 `.codex-dialog[role=dialog]` 没有 `aria-modal`；背景 utility 含 `/90`，有效声明为 `color-mix(...)`。适配器按实际 class 与有效 supports 声明提取，不将固定 token 名当样本。
- 已验证官方 class 换组、CSS 变量即时变化、portal 关闭后保留语义引用；临时 DOM 改动均恢复。
- 当前实窗命中背景、文字、边框和圆角。`--tw-shadow` 属于原元素局部机制，拒绝直接复制；按钮、次级文字等其他角色仍保留现有映射，不能称为全量动态取样。

外壳材质仅由 `.incodex-capture-dialog` 绘制；inspector 是透明布局容器，不重复叠加相同 alpha。独立颜色浮层仍需自己的表面，不能一并透明化。

编辑器以 `--incodex-capture-window-scale: 1.2` 仅扩展窗口宽高。字号、图标、按钮和间距直接复用原始 token，不参与倍率；非画布区域按 header/footer/toolbar 的既有间距预算，扩大的可用高度全部进入画布。实际布局参与 ResizeObserver/fit，不使用视觉 transform，保留视口 max-width/max-height 保护。

预览尺寸唯一来源是 `fitCanvas()` 计算的 canvas-frame；canvas 完整填满该框，禁止再设置 760px 或视口减常数的位图上限，否则成品在大预览区会缩窄且比例失真。

右侧背景设置栏宽度为 60 个 spacing 单位（默认 240px），比原先 224px 多 16px；窗口外框及控件密度不变，剩余横向空间由预览区占用。

当前桌面是本机资源来源，不是历史版本素材库或下载器：按钮位于既有“壁纸”标题右侧，点击后获取并应用当前桌面，复用原有 background 状态与图片 store，不新增独立系统壁纸组。不把系统素材嵌入 Runtime、不改变系统桌面设置。实验 adapter 仅通过现有 capture-debug open 连接，不代表安装态或 Windows 实机验收。

主机实现位于 crates/incodex-cli/src/system_wallpapers.rs，CDP 白名单路由位于 cdp_system_wallpapers.rs；system_wallpaper_catalog.rs 仅保留当前图片的现成缩略图候选，不再排序历史系统版本。macos_desktop_wallpaper.rs 在实验 open 主线程调用 NSWorkspace（主屏、首屏回退），把本地路径快照交给后台资源库；主机验证该文件后以 system-wallpaper-current 提供，不扫描历史版本或任意父目录。此 open 启动后修改系统桌面不会自动刷新快照。

ScreenKite 截图宿主取证确认的是当前桌面入口：后台 ImageIO 以长边 2600px 解码并编码 JPEG 0.85，再提供可选 ID。Shot 的 HEIC 适配采用该有界编辑图片语义，不声称保留原始 HEIC 像素。完整证据与历史目录缺口保存在私人文档 .internal-docs/shot/current-desktop-cross-platform-20260908.md；不要为凑五版本将 214×130 预览冒充完整原图。

透明标题动作（渐变展开/收起、获取当前壁纸）默认使用 text-secondary，hover 使用普通 text。primary-text 是实心主按钮背景的反色前景，不可用于透明动作，否则浅色主题白字落在白底上。

背景选择器图标继承控件文字色；未选中的自定义选色入口使用 surface-tertiary/text 成对语义，不把上次任意颜色与主按钮反色文字混配。选中自定义颜色时仍展示原色，既有隐藏图标语义不变。实心复制按钮与 toast 保留 primary/primary-text 成对语义。

背景选项统一为五列等宽正方形（aspect-ratio: 1），填满网格列并复用 radius-sm；纯色不再单独固定小尺寸。网格保留一个 spacing 的安全边距供选中环绘制，避免侧边裁切。
