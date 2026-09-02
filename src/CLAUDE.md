# src/
> L2 | 父级: ../CLAUDE.md

## 成员清单

CLAUDE.md: src 模块地图，维护直接成员与 runtime 子模块的职责边界。
build-runtime.ts: Runtime 构建器，将 Electron 源码与图标编译为可移植 `dist/` 资产并生成内容哈希清单。
compatibility.test.ts: 兼容性边界测试，防止已淘汰的产品实现重新进入运行路径。
deploy-runtime.ts: 本地 Runtime 发布器，以内容寻址目录原子切换 `~/.incodex/runtime/current.json`。
forensics.ts: 官方应用只读取证入口，解析 ASAR、plist、签名与安装状态。
forensics.test.ts: 取证逻辑的夹具测试。
instance-port-lease.test.ts: CDP 端口租约与进程归属测试。
instance.test.ts: 实例身份、锁与进程探测测试。
ipc-guard.test.ts: Electron IPC 来源认证测试。
locale.test.ts: 用户语言覆盖解析测试。
runtime/: Electron 注入与跨平台窗口生命周期实现；局部地图见 `runtime/CLAUDE.md`。
runtime-cleanup-owner.test.ts: Runtime 所有者退出与遗留会话清理测试。
runtime-late-recreation.test.ts: 会话关闭后迟到资源重建的回归测试。
runtime-load.test.ts: 外置 Runtime manifest、哈希与 fail-open 加载测试。
runtime-macos-update.test.ts: macOS Sparkle 自动恢复交接的同步启动与资产验证测试。
runtime-main-injection.test.ts: Electron 主入口结构契约，保护官方启动、注入与窗口生命周期边界。
runtime-main-session.test.ts: macOS 主进程会话行为测试。
runtime-manifest.ts: Runtime 资产目录与 manifest 写入规则的单一来源。
runtime-session-contract.test.ts: Rust 与 Electron 会话安全语义的对齐测试。
runtime-session-process.test.ts: 会话进程身份与清理测试。
runtime-windows-main-session.test.ts: Windows Runtime 主会话边界测试。
safe-home.test.ts: 隔离目录创建、复制白名单与销毁安全测试。
window-kind.test.ts: 官方主窗口与辅助窗口分类测试。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
