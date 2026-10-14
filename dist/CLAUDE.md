# dist/
> L2 | 父级: ../CLAUDE.md

成员清单
incodex-codex-mode.cjs: 官方模式就绪探测。
incodex-dock-menu.cjs: 原生 Dock 菜单与状态项。
incodex-inject.js: 共用私密窗口控件与遮罩。
incodex-instance.cjs: 实例所有权、raise 通道与端口租约。
incodex-ipc-guard.cjs: 跨进程请求守卫。
incodex-loader.cjs: ASAR 校验入口；显式安装探针不进入官方 main，普通启动保持同步放行。
incodex-macos-update.cjs: 官方更新桥接与原生权限任务交接。
incodex-main-actions.cjs: 可热换动作与权限呈现工厂，不持有授权事实。
incodex-main.cjs: 主进程编排；安装探针复用 nonce 回执后退出，原生 CLI 独占引导。
incodex-owner-core.cjs: 所有权证据与回收判定。
incodex-owner-recovery.cjs: 不确定所有权状态的安全恢复。
incodex-permission-copy.json: 共用65语言双权限文案、perm重入指引和系统页名模板。
incodex-permission-ui.cjs: 兼容原生权限引导与动画。
incodex-preload.cjs: 受限 renderer IPC。
incodex-runtime-load.cjs: 固定验证器与代际提交；权限业务复用已装协议。
incodex-safe-home.cjs: 隔离 home 的创建与焚毁。
incodex-window-kind.cjs: 窗口归属判断。
incodex-window-lifecycle.cjs: 窗口关闭与会话回收。
runtime-manifest.json: 绑定构建来源与发布资产摘要。

构建器从源代码生成产物。
禁止手改产物。
Loader 只分派入口，不推导授权状态。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
