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
